# Soak report JSON transport repair

## Scope and status

The user authorized this local reporting repair on 2026-09-14. This Pi session changed the report script and its regression script. The repair passed local verification. Shared inventory integration remains pending.

The reviewed base is `97e782fa4a7efe84d24cddf1ebd9cd6a35cd651a`. The source receipts identify working-tree bytes, not a new commit. Another agent changed the F1 diagnostic plan during this work. This session preserved that change.

D2/B44 containment and real-Docker verification remain assigned to another machine. This session did not change the driver, launcher, shared gates, claim inventory, workload, or harness pins. The 45-second finalization wait remains unchanged. This session did not launch Docker, infrastructure, an acceptance soak, or another agent.

## Defect and correction

The report script passed complete JavaScript Object Notation (JSON) documents through command arguments. Large documents exceeded the operating system argument limit. The report script then failed before it produced the required artifacts.

The script now uses `jq --rawfile` and `fromjson` for document inputs. Scalar metadata still uses the existing argument interface. Required parsing rejects malformed input and multiple JSON documents. Missing or empty optional inputs retain their prior no-data or bootstrap behavior.

Review of the first repair found three output-alias regressions. The baseline alias could produce a false pass. The passive alias lost measurements. The threshold alias prevented report generation. Separate public-script controls reproduced these regressions before correction.

The script copies external JSON inputs into a private temporary directory before it writes the affected outputs. Each consumer reads the captured input. The script removes the temporary directory on normal exit and handled failure. This does not guarantee cleanup after `SIGKILL`.

Report schemas, failure counts, verdict rules, and metric calculations remain unchanged. Active percentile figures remain medians of segment percentiles, not pooled percentiles. This repair does not add memory limits, disk quotas, atomic report publication, or authenticated input selection.

## Verification

The fixtures use the public report script and real `jq`. They do not replace internal collectors or report calculations. Synthetic JSON files form the input boundary. The checks include paths with spaces.

The frozen baseline failed four transport cases with `Argument list too long`. Each large input exceeds one mebibyte. The cases cover passive data, active data in the generated summary, baseline data, and threshold data. Large baseline and threshold inputs also test the generated verdict, badges, and Markdown report.

The final suite executes the public script 40 times. The suite includes the existing cases and these additional controls:

- Twenty-one controls cover missing, empty, null, whitespace-only, truncated, trailing-garbage, and multiple-document inputs.
- A checkpoint ignores an invalid baseline, as before.
- Three controls preserve inputs that refer to output paths.
- Five transport cases include the small-input control and four large-input cases.

The final frozen fixture fails against the original script and passes against the corrected script. Only the report script differs between those source sets. The working-tree suite also passes. The summary-writer regression passes.

The comparison checks 108 artifacts from 18 successful cases against the original script. Only generated dates and their Markdown lines are normalized. Two clock-dependent checkpoint reports remain outside this exact comparison. Their existing behavioral assertions pass. No temporary input directories remain after the final suite.

The historical archive digest was verified before retrieval. The replay used only report JSON inputs from run `34180346282`. The retained summary contains 164,253 bytes. The original script fails on that summary. The corrected script produces all six report artifacts.

The historical report retains 51 iterations, one failure, provider data, and all tracked metrics. Its verdict remains `regress`. This replay did not rerun the workload or establish soak acceptance.

Shell syntax, whitespace, protected-input hashes, and warning-level language-server checks passed. Unchanged `$ba` identifiers still produce informational spelling findings. These findings do not identify shell defects.

## Evidence and unsuccessful attempts

The retained evidence directory is:

```text
[EVIDENCE_HOST]/soak-evidence/f1r3node-rust/report-json-transport-p4EMyozg
```

The principal records are:

- `integrated-pair-inputs.json` records the final source digests and sizes.
- `integrated-pair-results.json` records the final baseline and candidate commands and exits.
- `red-v7.stdout` and `red-v7.stderr` retain the final baseline result.
- `green-v6.stdout` and `green-v6.stderr` retain the unchanged-fixture passing result.
- `alias-red-source.json` and `alias-red.*` retain the first two output-alias regressions.
- `passive-alias-red-source.json` and `passive-alias-red.*` retain the passive alias regression.
- `historical-v2-retrieval.json` identifies the archive and retrieved files.
- `historical-v2-results.json` records the final historical comparison.
- `small-report-equivalence.json` records the artifact comparison and its exclusions.
- `integration-bindings.sha256` supplies the two reporting input digests.

Earlier attempts remain retained. An unsupported snapshot option and an incorrect snapshot path prevented setup. A copied read-only fixture prevented a control from changing its own input. Another control incorrectly expected null thresholds to pass with a baseline. These attempts are not behavioral RED. Corrected fixtures received fresh baseline and candidate comparisons.

## Integration boundary

The shared inventory check fails because its reporting input binding is stale. The integration agent retains ownership of the inventory. The integration agent must review and update these two bindings:

```text
scripts/bench/aggregate-perf-report.sh
scripts/bench/test-aggregate-perf-report.sh
```

Digest updates must not change claim or gate statuses. The two scripts have `cbc=unspecified`. This attribute result does not discharge a correctness claim. No applicable argument-transport model was identified or executed. These results are executable regression evidence, not formal proof.

O1 completion, containment, reserve bounds, finalization verification, shared checks, and acceptance remain pending. This session did not stage, commit, or push changes during implementation.

## Commit review and metric correction

The user subsequently requested staging and a commit. Commit review found incorrect instrumentation claims in the staged F1 plan. The user authorized their correction before the commit.

The corrected plan identifies the existing carrier counters in validation and the replay input-count histograms. It distinguishes watermark eligibility from scan avoidance. It distinguishes traversal operations from physical storage reads and unique ancestors. Replay input counts do not establish successful completion or per-block attribution.

The review also removes unsupported coverage claims for queue emission and inode sampling. The plan requires verified collection and bounded block correlation before new instrumentation. The correction changes documentation only. It does not complete O1 or authorize a diagnostic run.

Formatting changed the reporting source and fixture after the original verification. The commit review therefore retained a fresh source-bound comparison. The formatted fixture reproduces four argument-size failures against the pre-repair script. All 40 cases pass against the formatted candidate. The historical replay and summary-writer regression also pass.

The additional evidence directory is:

```text
[EVIDENCE_HOST]/soak-evidence/f1r3node-rust/report-commit-review-VZSFj5pI
```

`reviewed-source.json` binds the node source used for the metric correction. `reporting-inputs.json` supplies the current reporting digests and supersedes the earlier digest handoff. `results.json` records the new report comparisons and summary-writer result. The shared inventory still requires the integration agent's update. These checks do not discharge claims or establish acceptance.

## Production evidence and pull request preparation (2026-09-16, claude-session-c942697b)

The scheduled soak run 34921498873 of 2026-09-15 hit this defect in production. The run targeted `ffefd6932` on `dev` under the master workflow at `adcc3fa74`. Its checkpoint aggregation failed twice with `jq: Argument list too long` at line 174 of the report script. Both `weekly-summary.json` files in its artifact are empty. The run's summary holds 164196 bytes and 51 iterations.

The failure is Linux-specific. Linux caps one argument string at 128 KiB, and macOS does not. The report script passed the whole summary as one argument, so the same input passes on a developer Mac and fails on the runner.

The integration session reproduced the failure and the repair on 2026-09-16 with the real artifact:

| Check | Script | Result |
| --- | --- | --- |
| Regression suite on macOS, bash 5.3, jq 1.7 | `dev` at `4d8d9d79c` | Exit 1. Four cases fail with `Argument list too long`. |
| Regression suite on macOS | Staging at `10a29853f` | Exit 0. All cases pass in under four seconds. |
| Real artifact in the CI fixture image, Debian, jq 1.6 | `dev` at `4d8d9d79c` | Exit 126 at line 174 with `Argument list too long`. The output directory holds one empty `weekly-summary.json` and nothing else, the same as the production artifact. |
| Real artifact in the CI fixture image | Staging at `10a29853f` | Exit 0. Six artifacts, verdict `regress` with one active failure. |
| Real artifact on macOS | `dev` at `4d8d9d79c` | Exit 0, since macOS has no per-argument cap. This is why local checks did not catch the defect. |

The script digests are `4205c691` for the report script and `ff010f3d` for its regression script, the same as the claim inventory bindings. The raw evidence is session-local and is not committed.

### Pull request

The repair goes to `dev` as its own pull request, independent of the disk-hygiene stack. The maintainer creates the branch and the pull request. The files are the two scripts and this work log:

```text
scripts/bench/aggregate-perf-report.sh
scripts/bench/test-aggregate-perf-report.sh
docs/work-logs/task-soak-report-json-2026-09-14T17-35Z.md
```

The F1 plan correction from the same staging commit stays out. That plan file does not exist on `dev`. The summary writer also stays out, since its staging changes belong to the driver track.

Suggested branch and title:

```text
fix/soak-report-json-transport
fix(soak): pass report JSON to jq through files, not arguments
```

Suggested description:

> The soak report script passed complete JSON documents to `jq` as command arguments. Linux caps one argument at 128 KiB. The scheduled soak run 34921498873 of 2026-09-15 crossed that cap with a 164 KB summary after 51 iterations. Its checkpoint aggregation failed twice with `Argument list too long`, and its artifact holds two empty weekly summaries and no verdict.
>
> The script now reads document inputs with `jq --rawfile` and parses them with `fromjson`. Scalar metadata keeps the argument interface. Required inputs reject malformed or multiple documents, and empty optional inputs keep their no-data behavior. The script copies external inputs into a private temporary directory before it writes outputs that alias them. Schemas, failure counts, verdict rules, and metric calculations are unchanged.
>
> The regression script gains 21 malformed-input controls, three alias controls, and four large-input cases over one mebibyte. The four large cases fail against the current `dev` script and pass against this one. The CI step `Verify soak verdicts` runs the suite on this pull request. The real artifact of run 34921498873 fails against the `dev` script inside the CI fixture image and produces all six report artifacts with this script. The work log records the evidence.

