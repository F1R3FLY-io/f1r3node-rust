---
task: TASK-017-4
cycle: H10-exact-result
claimed_by: pi-casper-harness
handoff_status: partial_cycle_verified
construction: not-applicable
---

# H10 Exact Result Classification

The user authorized continued H10 implementation after staging the previous changes. This cycle changes the shared formal gate and its executable fixture.

The starting revision was `4204340b0d573e14df9b224ae43e0c0aea083136`. The staged files included the previous positive-search cycle.

The index matched its initial snapshot when the evidence package was created. A concurrent Git operation later advanced HEAD to `68db00227656a51013580c81dd43f02e7c00327e` and staged this cycle.

This assistant preserved those Git operations. The tested source digests still match the working files.

## Behavior

A positive result requires exit zero, the completed-search marker, and no TLC error line.

A negative result requires exit 12, exactly one expected invariant violation, and a counterexample state. It cannot contain a completed-search marker or an unexpected error line.

The parameterized regression covers one contradictory positive result and four ambiguous negative results in each of three registered areas. The negative cases contain:

- A completion marker beside the expected violation.
- An additional invariant violation.
- A duplicate expected violation.
- An additional verifier error.

The fixture substitutes only the external verifier process. Each case invokes the production gate through the pull-request workflow command.

## Results

- RED exited 1 because the previous gate accepted the contradictory positive result. This failure established the intended defect.
- GREEN exited 0 with all 13 new ambiguity cases rejected. Each rejection identified the intended configuration.
- The existing 21 negative rejection cases and two positive rejection cases also passed. All 61 registered negative controls retained acceptance of valid results.
- Tier routing and registration checks passed.
- The bounded shared TLC tier passed 13 positives and 61 negatives in 67.025 seconds, including container setup.
- The Casper clean configuration and ten named negative controls passed. The clean model explored 43,424 distinct states.
- All twelve Casper runner tests passed. Shell syntax checks and active language-server checks passed for both changed scripts.
- The strict epic CbC gate returned exit 4. No full claim was discharged or waived.

The [report](../casper/cbc-evidence/runs/casper-exact-result-20260917-01/report.json) identifies 127 retained records and 182 source digests. The artifact bundle stores transcript contents with original and retained SHA-256 values.

The bundle also contains RED/GREEN source snapshots, tool versions, and the validation helper. The report retains seeds, state counts, limits, and the immutable container image identity.

The cycle patch starts from the retained before-source snapshots, which include previously staged changes. It does not assume a clean starting checkout.

The containers used an unprivileged account, no network, and no host mounts. Each stopped without an out-of-memory event before removal.

Per-configuration exit values derive from successful gate branches. They are not independent exit-code files.

## Limits and Next Step

The shared result classifier passes the stated fixtures. H10 remains partial because shared Casper registration and full driver/evidence bindings remain pending.

The accepted TLC text format remains an assumption. This cycle does not prove arbitrary-output parsing or node correctness.

The next step is to register the Casper controls in the shared gate without weakening their finite bounds or timeout requirements.

No node workload ran. The candidate matrix, external dependency pin, runtime code, and mandatory-file inventory remain unchanged.

The repeated native-fixture findings remain platform and scanner false positives. Fresh Linux-targeted Pyright passed, and the reviewed entrypoint catches parser exceptions.

The Boolean identity check deliberately excludes other false-like values. No native fixture changed to silence host diagnostic cache entries.

The deterministic STE Check does not replace human STE Review. This assistant did not stage, commit, push, merge, or repin dependencies.
