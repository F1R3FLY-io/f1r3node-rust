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
/home/bf_spark/soak-evidence/f1r3node-rust/report-json-transport-p4EMyozg
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
/home/bf_spark/soak-evidence/f1r3node-rust/report-commit-review-VZSFj5pI
```

`reviewed-source.json` binds the node source used for the metric correction. `reporting-inputs.json` supplies the current reporting digests and supersedes the earlier digest handoff. `results.json` records the new report comparisons and summary-writer result. The shared inventory still requires the integration agent's update. These checks do not discharge claims or establish acceptance.
