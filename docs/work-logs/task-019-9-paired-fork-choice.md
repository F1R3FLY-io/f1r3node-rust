# TASK-019-9: Batch D, paired fork-choice observation bound to one capture

## Session

- Implementer: `claude-session-f3cbc961` (agent A of the two-agent split of 2026-09-30).
- Claimed: `2026-09-30T04:40:00Z`.
- Branch: `feature/casper-node-observation`, head `e90e4cffa` at the claim.
- Epic: EPIC-019. Claim: `CLAIM-CASPER-NODE-OBSERVATION-004`, not registered yet.
- Consumer: TASK-017-12 on `formal/soak-casper-consensus`.
- Plan draft: `target/node-observation-prep-20260928-01/batch-d/` (local, ignored by Git). The accepted plan moves to `docs/plans/casper-node-observation-batch-d.md` at the file-scope confirmation.

## Split with agent B

Agent B (the pi session of 2026-09-30, [its work log](node-observation-agent-b-20260930.md)) owns the record refresh of the 10 evidence records, the STE fix in `docs/User-Flows.md`, the TASK-019-8 inventory, and Batch E after Batch D. Agent A owns Batch D and then the removals of TASK-019-8.

Rules: one agent edits `docs/ToDos.md` at a time. Each agent stages only its own files. The user makes each commit with `/quick-commit`.

## Gate before code

Steps 1 to 4 of the plan must be complete before a production file changes:

1. File-scope confirmation of the 34 files (17 new, 17 changed) and the answers to the 12 open questions.
2. Task record and work log (this file).
3. Pending claim registration and the mandatory tags.
4. Pending ledger records for the mandatory files.

## Draft currency check on 2026-09-30

The draft has the reviewed base `670037c25`. At `e90e4cffa` the 16 existing files of the plan have no change since that base (`git diff --stat 670037c25 HEAD -- <files>` is empty). The source findings of the draft hold: `evaluation.rs` has no estimator reference, `observation_work.rs` has 4 work paths, and no `fork_choice` symbol exists in the observer files. The CbC tags agree with the draft table.

## Gate 1 to 4 on 2026-09-30

The user confirmed the 34-file scope and accepted the 12 recommendations of the draft (question 8: the 3 consensus files get the mandatory tag and 3 ledger records). The decisions are in the [plan](../plans/casper-node-observation-batch-d.md).

Registered: the [plan](../plans/casper-node-observation-batch-d.md), the pending [claim](../claims/casper-node-fork-choice-observation.md) `CLAIM-CASPER-NODE-OBSERVATION-004`, 4 mandatory tag lines in `.gitattributes` (`fork_choice.rs`, `estimator.rs`, `dag_operations.rs`, `proto_util.rs`), and 14 pending ledger records with the scope `batch-d-registration`.

| Record group | Count | Note |
|---|---|---|
| Replaced records of changed files | 11 | `previous_record` names the replaced accepted record and its digest |
| New records of the newly tagged files | 3 | `sha256_at_registration` holds the digest before the change |
| New records of new files | 0 now, 14 at step 5 | Written when each file exists and has a digest. The user chose this on 2026-09-30 to limit empty files on the branch. The registration commit `bc6e2f3d4` had the 14 placeholders, and the next commit removed them. |
| Kept under another claim | 1 | `scripts/ci/check-tla-invariants.sh` stays under `CLAIM-SOAK-GATE-001`, as at the Batch B2 registration |

The record generator is `target/batch-d-registration-20260930-01/write-pending-records.sh` (local, ignored by Git). Each record parses as JSON.

## Step 5 on 2026-09-30: bounded model

`PairedForkChoice.tla` models one capture, the 2 evaluations, the shared budget, and the comparison. Domain: 2 captures, 2 candidate heads, budget 3. The positive configuration explores 203 distinct states.

| Control | Invariant | Result |
|---|---|---|
| `digest_unsafe` | `OneCapture` | Exit 12, 21 states |
| `floor_unsafe` | `HeadNotFloor` | Exit 12, 17 states |
| `absent_unsafe` | `NoFabricatedHead` | Exit 12, 17 states |
| `compare_unsafe` | `CompareSameInput` | Exit 12, 113 states |
| `budget_unsafe` | `SharedBudget` | Exit 12, 29 states |

The first `compare_unsafe` control did not fail, because equal inputs cannot show a missing digest check. The control now also lets the reference read a different capture. A reachability check with 6 negated invariants ran on the positive model. The model reaches a matching comparison, a mismatching comparison, an unavailable comparison, a limit refusal, a history refusal, and a full budget.

Registration: `check-tla-invariants.sh` (default tier, PR tier, 5 controls, plan count 19 to 25, third positive model), `test-check-tla-invariants.sh` (family loop), `verification-plan.json` (6 entries), `README.md` (inventory and model section). `bash scripts/ci/check-tla-invariants.sh --soak-pr` passed in 72 s with 16 clean configurations and 83 controls. `bash scripts/ci/test-check-tla-invariants.sh` passed in 145 s. The 13 ledger records of the model files are written with their digests.

## Step 6 on 2026-09-30: two work paths

`shared/src/rust/dag/observation_work.rs` gets the constant `WORK_PATHS = 6`. The budget state, `for_path`, and `usage` use the constant. Path 4 is the `bounded` fork-choice evaluation and path 5 is the `reference` evaluation, both unused until steps 9 and 10.

The Rocq work model `AuthorityWork.v` has no path count. Its theorem `paths_share_one_budget` is generic over charge lists, so the conditional change does not apply.

New test in `casper/tests/soak_observer.rs`: `work_budget_has_six_paths_and_the_aggregate_is_their_sum`. It charges each of the 6 paths, refuses path 6, and checks that the aggregate is the sum of the paths.

Checks: `cargo nextest run --locked --release -p shared -p casper -p node` for the `soak_observer` and `soak_snapshot` binaries and the `shared` package, 158 of 158 passed. `cargo clippy --locked --release -p shared -p casper --tests -- -D warnings` passed. `rustfmt --check` passed on the 2 files.

## Step 7 on 2026-09-30: metered entry points

Each of the 4 functions keeps its body in a `_metered` variant, and the current function calls that variant with `NoopWork`. This is the Batch B2 pattern of `floor.rs`.

| Function | Charge sites |
|---|---|
| `proto_util::weight_from_validator_by_dag_metered` | `lookup()` before each of the 2 DAG reads |
| `DagOperations::lowest_universal_common_ancestor_many_metered` | Allocation for the sets, a traversal step for each loop iteration, `lookup()` or a traversal step in the cache helper, allocation for each cache insert |
| `Estimator::tips_with_latest_messages_metered` | Metadata charges for the batch reads and `lookup()` before each DAG read. A traversal step for each BFS pop, queued parent, child scan, and descent step. Allocation for each visited and score entry. One traversal charge of `n` for each sort. |
| `floor::fork_choice_floor_metered` | A traversal step for each latest message, `floor_of_block_metered`, `lookup()` before the final read, and one traversal step after the loop |

The step after the loop in `fork_choice_floor_metered` makes a budget failure visible. The loop reports a non-held error of one latest message as an abstention and continues. A budget failure inside `floor_of_block_metered` is such an error. The meter failure is sticky, so the final step returns it. With `NoopWork` the step is a no-op.

The frontier filter of `rank_forkchoices` became a loop, because the child scan now returns a `Result`. The set of frontier hashes, the distinct step, and the sort call did not change.

4 differential tests in `casper/tests/soak_observer.rs` run on a 5-block fork DAG. Each metered function gives the result of the current function for several inputs. The counted work is above zero, and the missing-block error text is equal. A budget of 1 operation gives the `observation_work:` limit error.

Checks: 95 integration tests of the fork-choice, estimator, floor, and observer suites passed. 65 unit tests of the casper library for floor, estimator, and weight passed. `cargo clippy --all-targets -- -D warnings` for `casper` and `node` passed. `rustfmt --check` passed on the 5 files after a format pass on 2 of them.

## Step 8 on 2026-09-30: input record and input digest

`CaptureEndpoint` binds `ForkChoiceInputs` at attachment: `max_number_of_parents` from the shard configuration, `approved_block_number` from the approved block, and `latest_message_depth` from the estimator constant, which is public now. Two accessors give the tests read access to the authority and fork-choice inputs.

`AuthorityRequest` gets the optional field `fork_choice: Option<ForkChoiceSelection>`, absent from the JSON when `None`. `ForkChoiceSelection { reference: bool }` rejects unknown fields.

The authority digest input is a struct of the 8 Batch B2 request fields, plus the selection only when it is present. `bincode` writes that struct and the request struct identically, so the digest of a request with no selection is byte-equal to the Batch B2 digest. A test compares against a literal copy of the Batch B2 digest input.

The fork-choice input digest has the domain `batch-d-fork-choice-v1` and covers the authority digest, the inputs record, the scope `captured_latest_messages`, and the 2 lower bound rule names. `fork_choice_input_digest` is public for the tests.

The response has the field `fork_choice: Value<ForkChoiceObservation>`. With no selection it is `NotRequested`. With a selection the observation is available with the input digest, the inputs, and the captured latest message count. Its `bounded`, `reference`, and `comparison` fields are `Unavailable` with the reason `not_implemented` until steps 9 to 11.

`casper/src/rust/soak_observer/fork_choice.rs` holds the result types only: `ForkChoiceObservation`, `ForkChoiceResult`, `ForkChoiceComparison`, `LowerBound`, `LowerBoundRule`, `LatestMessageCounts`, `EvaluationMode`, and the constants. Its ledger record is written now, because the file exists.

Checks: the 24 casper observer and estimator tests passed, which include the 2 new tests. `node/tests/soak_observer.rs` is Linux only (`#![cfg(target_os = "linux")]`) and runs in CI. `cargo clippy --all-targets -- -D warnings` for `casper` and `node` passed. `rustfmt --check` passed on the 5 Rust files.

## Progress

- [x] Claim the task and start the work log.
- [x] File-scope confirmation and the 12 answers.
- [x] Pending claim, mandatory tags, pending ledger records.
- [x] Bounded model and 5 controls, registered in the gate.
- [x] 2 work paths.
- [x] Metered functions in the 4 consensus files.
- [x] Input record and input digest.
- [ ] `bounded` evaluation and tests.
- [ ] `reference` evaluation and tests.
- [ ] Comparison, response field, negative controls.
- [ ] Capability entry and node tests.
- [ ] Binding entries and applicability review.
- [ ] Gate, binding check, test suites, results recorded.
- [ ] Compact evidence package and the acceptance request.
