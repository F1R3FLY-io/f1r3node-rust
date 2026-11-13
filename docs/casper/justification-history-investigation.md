# PR #1009: historical justification rejection

## Build provenance

The failed [ARM64 subprocess job](https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/43297612473/job/131476585681) tested synthetic merge `d8feacb2866ee754ea5851f52336577d4a438de2`, not PR head `52763ce3f4a91adb4429d04b901f415b1aae41d1` alone. The base was `67bc1ccc4ec51ce1c5946780d2b74d72a3909b71`. The build identifier was `v0.4.46-canary.5021-130-gd8feacb28`. The harness pin was `cbda724b0b240dec701f82ecc5837de22c59a7c1`.

Evidence comes from artifact `integration-logs-arm64-subprocess-run43297612473-attempt1`, its `integration-tests.log`, and `log-archive/984218f5/shard19/`. The relevant validation, worker, ingest-state, dispatcher, and block-status source blobs in the tested merge match the checkout before this fix.

## Block history

The full candidate hash is `119f3c4ec41d5c0114aade26f8db12bc78c814b7d0b04044b72e20652df20f84`.
Its sender is `04801118220021f056f4bc46340eebbaf0981ed55de15b02a9da96219d572409af1bdc1dfc80c5eb74b36597f7d209997813b15e776859c9750b5a02fb195ead06`.
It has sequence number 6, height 18, timestamp 1794518209484. The gRPC block-info dump begins at line 150686 of `integration-tests.log`.

Its ordered parents are:

1. `4cf3b33e899cb14e2af44408e228e443847ba504331b5fb5e95928e7667efa82`
2. `6e8346694ee82b1fcc51ae8744bd2ce5d6f79a4054a4f1e8e369c9d8ad0e170a`
3. `99b2753a0c673f5c4798468d4418468ef26bb063406818bde13ceaf528515cbf`
4. `c5c4ff3dba9d0578c66e330c5fdd32599f575bd193fdded5e44032d04c8ec877`

Its justifications cite those same four blocks, keyed by their respective validators. The signed predecessor is parent 1, sender sequence 5 at height 17. The table compares its justifications with the candidate's:

| Validator prefix | Predecessor witness | Sequence | Candidate witness | Sequence |
| --- | --- | ---: | --- | ---: |
| `0480111822` | `ac712c78c28f12938abe782a13f77ef8aee17b58bcd797556d747200a0115080` | 4 | `4cf3b33e89…` | 5 |
| `0457febafc` | `5c6b0d7a243001f9d11795992660c0beede3c38abb9dc43a862dac8ac60c565c` | 9 | `6e8346694e…` | 10 |
| `04837a4cff` | `735e8c4c7811bcb0f313c468d152f799cc7d34b6020c02cfb22b067222a63f64` | 10 | `99b2753a0c…` | 11 |
| `04fa70d7be` | `a3f82a7efcb74e1ab731d619af10c3f79b90706eedc0bb9037d78c294f243d8d` | 11 | `c5c4ff3dba…` | 12 |

Every entry advances. This is not a genuine justification regression.

## Arrival and validation order

All times below are UTC on 2026-11-12.

| Node | Candidate validated as valid |
| --- | --- |
| validator1 | 21:16:49.925017 |
| validator3 | 21:16:49.939523 |
| validator2 | 21:16:49.941287 |
| boot | 21:16:49.957162 |
| readonly | 21:16:49.975868 |
| joiner6 | 21:16:49.978659 |

Joiner8 attached at an anchor of height 105 and processed newer messages before fetching the historical candidate. It validated `c579f5ad66776b94ba55ff4ba575095f00922624396dfac92b452ac584d145aa`, sender `0457febafc…`, height 143, at 21:25:07.204786. It then processed candidate-sender block `ca9fb946d6f7848d41fbc6b8e6a053f6f911013cdc338c5cffb1762c78323d6d`, height 144, and validated it as valid at 21:25:14.690285.

At 21:25:18.233303, .455985, .496591, and .535261, joiner8 admitted the four candidate parents as settled history. Candidate validation began at 21:25:18.554371. The scanner warning at .560775 followed a comparison of candidate witness `6e8346694e…` against later witness `c579f5ad66…`.

`Validate::justification_regressions` selected `s.dag.latest_message(&b.sender)`. Thus the newer local sender view supplied the comparison rather than the candidate's signed sequence-5 predecessor. This establishes an arrival-order false rejection.

The logs do not dump every local latest-message table mutation. The exact entry is inferred from the matched implementation and the preceding accepted sender history. It is not established by a table snapshot.

## Duplicate rejection and effects

The first attempt returned `Invalid(JustificationRegression)` at 21:25:18.560807. Ingest logged `reason=invalid, dropped=4`: it removed the candidate's pending body and three held dependents. Heartbeat and descendant validation still needed the block. The second delivery repeated validation at 21:25:21.062625 and the warning at .068809. These are separate attempts, not duplicate collection of one log line.

`JustificationRegression` is non-slashable. The default branch of `dispatch_handle_invalid_block` logs “Recording invalid block” but returns the unchanged DAG. It creates neither invalid metadata nor an equivocation record.

`judge_invalid` announces the hash even on that branch, then reports rejection because the hash is absent from the DAG. The two hash announcements are visible at 21:25:18.601586 and 21:25:21.110424. No slash evidence was created by either rejection.

At 21:25:22.657549 the block was processed a third time. At .659297 it was admitted through the existing settled-history path.

The waiting height-145 blocks resumed at .659603 onward, requesting older missing history next. This explains how all 14 functional phases could complete despite the warnings.

Later settled-history admission does not make either earlier verdict correct. The two attempts caused temporary ingestion disruption. They did not persist an invalid flag or permanently poison this block's DAG entry.

## Fix and verification

The fix selects the comparison block through the candidate's signed creator justification. It keeps all-validator comparisons, the invalid-witness exemption, and the preceding sequence check. Absent predecessor metadata or comparison witnesses now return `Undecidable(hash)` so ingestion can fetch them.

The deterministic catch-up regression fails against the original implementation when later sender messages arrive before the historical candidate. A genuine regression still fails before and after that arrival. The missing-evidence test also fails before the fix and checks deferral followed by rejection after hydration.

Validation results are recorded in the [claim](../claims/justification-history.md) and active CbC evidence ledger. The original diagnostics ledger is retained byte-for-byte in evidence history. Existing signed cleanup acceptances are unchanged.

No system-integration change is required for this demonstrated cause. PR #182 fixes a separate Docker lifecycle defect. No expensive integration suite or remote run was started. The fixed node still needs lifecycle integration validation through the normal review and CI process.

The historical local latest-table entry is inferred as described above. The valid predecessor-relative comparison is directly supported by the block dumps.

## Validation results

| Check | Result |
| --- | --- |
| Original validator with the two new regressions | Both failed, including the arrival-order false rejection |
| `cargo test --locked -p casper --test mod justification_regression -- --nocapture` | 5 passed, including the strengthened hydration check |
| `cargo test --locked -p casper --test mod batch2::validate_test -- --nocapture` | 58 passed |
| `cargo clippy --locked -p casper --tests -- -D warnings` | Passed |
| Targeted `rustfmt --check --edition 2021 --config skip_children=true` and `git diff --check` | Passed |
| Offline Markdown link checks | Passed |
| TLC clean configuration | Exit 0, 5,184 distinct states, depth 3 |
| TLC old local-latest comparison | Expected exit 12 on `HistoryRelative`, with complete witnesses |
| Rocq 9.1.1, fresh slashing project build, `make -j1` | Passed. Four added theorems closed under the global context |
| `coqchk -Q theories Slashing Slashing.MainTheorem` | Passed, including all four added theorems |

Rocq ran in a temporary copy containing source files only. The checked `BugFixSelfRegression.v` and TLA+ model match the working-tree sources byte-for-byte. No pre-existing compiled proof objects were used. Local test logs remain under `/private/tmp/justification-*`. The retained evidence report records commands, results, source digests, and log digests.

The active CbC claim remains pending specification and Rust-binding review. No maintainer acceptance was signed or fabricated. The previous diagnostics ledger has SHA-256 `f373d6ce5659b6b342c6e7f3e5993b84d770e0e108c24f58a080ac5cfd745f58` and remains preserved in evidence history. The existing signed cleanup acceptance files are unchanged. This change does not renew a historical acceptance of the validation artifact.

The archived ledger is a byte-for-byte historical copy. Its relative links retain the original ledger's base directory. Exclude only this archived file from current-path link checks. Its digest and contents must still match the prior ledger.
