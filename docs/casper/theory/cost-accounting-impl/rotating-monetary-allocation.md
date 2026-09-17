# Rotating monetary allocation

## Status and scope

The user selected the planned rotating residual policy on September 9, 2026.
This decision rejects the historical canonical-first fee fallback.
The user also approved equal monetary fee treatment for authorized joint and individual purses.
The joint purse has no automatic priority over individual purses under this policy.

The numeric allocator and its focused checks are implemented.
The native fee paths now call this allocator through a shared planner.
Both [multi-wallet regressions](../../../../casper/tests/util/rholang/multi_payer_fee.rs) passed after integration on September 10, 2026.
They check one total fee, equal cumulative rotation, certificate cursor transitions, and native/replay root equality.
This document does not claim that the fee repair or the complete funding solver is finished.

Voting, fork choice, finality, and pruning remain outside this repair.
Logical authority multiplicity remains distinct from the conserved monetary obligation.
The new allocator does not change compute authority or byte authority rules.

## Numeric contract

A payer cohort is the ordered set of eligible funding identities for one allocation scope.
A capacity is the permitted monetary debit after other reservations and charges.
A residual is an indivisible unit left after equal allocation subject to capacity limits.
A cursor identifies the first cohort position considered for residual allocation.

The caller must authenticate the cohort and establish its canonical order before invoking the numeric allocator.
The caller must obtain capacities and the cursor from certified state.
The numeric allocator does not authenticate identities, resolve custody aliases, read balances, or write state.

Let $`n`$ denote the positive payer count, $`C_i`$ each capacity, $`A`$ the monetary obligation, and $`s`$ the input cursor.
Indices range from zero through $`n-1`$.
The configured positive cap bounds $`n`$.
The implementation rejects an empty cohort, an excessive cohort, an invalid cursor, and insufficient aggregate capacity.

Define the common level $`h`$, base allocations $`B_i`$, and residual count $`r`$:

```math
h = \max\left\{q \in [0,A] : \sum_{i=0}^{n-1}\min(C_i,q) \le A\right\},
\qquad B_i = \min(C_i,h),
\qquad r = A - \sum_{i=0}^{n-1} B_i.
```

Only positions with $`C_i > h`$ can receive an extra unit.
The allocator scans those positions cyclically from $`s`$ and gives one extra unit to each of the first $`r`$ positions.
The output cursor follows the last extra recipient.
If no extra unit exists, the cursor remains unchanged.

The algorithm separates equal base allocation from residual selection:

```text
Validate the cohort size, cursor, and available capacity.
Find the largest feasible common level with binary search.
Assign each payer the smaller of its capacity and that level.
Distribute residual units in cyclic canonical order.
Return the debit vector and the next cursor without mutating state.
```

For a fixed one-unit fee, the first positive-capacity payer from the cursor pays the single unit.
Zero-capacity positions remain in the cohort but receive nothing.
If only one position has capacity, that position receives the fee as a base allocation, and the cursor remains unchanged.
Thus successful settlement advances this residual cursor only when a residual exists.
For capacities `[0, 9, 0, 9]`, obligation `3`, and cursor `2`, the output is `[0, 1, 0, 2]` with cursor `0`.

The [Rust kernel](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation.rs) uses unsigned 64-bit amounts and checked 128-bit aggregates.
Binary search needs at most 64 iterations for these amounts.
Each iteration scans the cohort, and residual distribution needs at most one additional cohort scan.
Its time bound is $`O(n\log(A+1))`$ and its output space bound is $`O(n)`$.
It does not loop once per monetary unit.

## Atomic settlement requirements

### Authorized physical cohort

The [fee filter](../../../../rholang/src/rust/interpreter/accounting/authority/monetary.rs) takes the authenticated funding event as its authority source.
It does not derive fee authority from arbitrary program, compute, or byte events.
The caller remains responsible for authenticating the deploy that supplies the funding event.

The filter includes selected signer leaves, their full joint signature, and explicitly presented signatures whose nonempty atom multisets fit the funding authority.
An atom is one indivisible logical authority component.
Multiplicity matters: a signature containing Alice twice is not authorized when the funding authority contains Alice once.
Unrelated presentations remain available to other accounting paths, but they cannot pay the fee.
The filter does not enumerate a powerset of possible joint purses.
Existing host-work checks bound signature discovery before the filter constructs the result.

The [cohort builder](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/cohort.rs) groups eligible lanes by their existing validated physical custody bindings.
Each distinct physical purse receives one monetary allocation position.
This grouping does not identify different logical authorities with each other.
It retains eligible logical lanes and uses their canonical first lane to express the selected physical debit in existing settlement evidence.
That representative does not determine the purse's monetary share.

The builder requires a nonempty cohort, matching lane signatures, known custody bindings, and a configured physical-purse cap.
It does not derive vault addresses or authenticate custody bindings itself.
Native callers must use the existing `vault_payer` and `insert_physical_balance` checks before constructing the cohort.
The cohort order follows physical custody keys and includes zero-balance purses.
Its scope hash binds the policy context and the ordered physical identities, not balances or alias counts.
The native integration defines and validates the policy context, including its asset, custody role, and allocation version.

The signer cap and physical-purse cap are different limits.
Sixty-four signers plus their joint purse can require 65 physical-purse positions.
Explicitly presented joint subsets can add positions within a separately checked bound.
The numeric kernel accepts a configured positive cap and has no built-in two-payer or 64-purse restriction.

The eligibility model checks every nonempty selection over three atoms against all multiplicity vectors from zero through two.
Its safe instance and two negative controls passed before the Rust fee filter was added.
The controls reject unrelated authority and set-only comparison that discards multiplicity.
Five focused eligibility tests and eight physical-cohort tests passed, including two generated properties with 256 cases each.
The 64-signer case also constructs and funds the 65-position cohort, including its joint purse.
Other cases reject unrelated funded inventory and empty authority, and detect scope changes when a zero-balance purse joins.
Strict Rholang and Casper library and test Clippy checks passed.

[`FeeCohortEligibility.v`](../../../../formal/rocq/cost_accounted_rho/theories/FeeCohortEligibility.v) proves exact list-multiset containment and sound, complete presentation filtering.
It also proves that excessive multiplicity is rejected and that unrelated presentations do not change eligible membership.
The five headline theorems compiled and passed an independent kernel check without additional axioms.
These proofs abstract canonical signature parsing, cryptography, physical custody resolution, and native state publication.

Five [Casper identity tests](../../../../casper/src/rust/util/rholang/acceptance/tests/monetary_tests.rs) passed with authenticated version-six threshold envelopes and actual vault custody resolution.
They check equal joint/individual rotation, stable cohort identity after a joint-purse top-up, and exclusion of unsigned members.
An explicitly presented authorized joint subset receives one equal payer position.
A separate custody test confirms that legacy and principal aliases share one physical position.
That custody test supplies eligibility directly and does not claim that a version-six envelope authorizes both aliases.
The unsigned-member property uses 64 generated selections.
These tests do not execute `SystemVault.applyCost` or prove cursor persistence.

### Commit boundary

The next cursor is a proposed transition, not permission to update storage during planning.
A successful settlement must commit the monetary changes, cursor transition, receipt, and user state together.
An aborted attempt must preserve the prior committed cursor.
An already settled transaction must not settle twice.

A balance check that only confirms sufficient funds is not sufficient to validate a stale allocation.
Another settlement can change the eligible payer or cursor while the old payer still has enough funds.
Validation must cover the certified causal pre-state and the cursor transition used by the plan.

The existing native write path is `ApplyCostDeploy`, `SystemVault.applyCost`, reservation, settlement, and the candidate checkpoint.
This path is the integration candidate, not a completed concurrency guarantee.
The contract currently accepts `reservationId` without using it as a stale-plan guard.
A separate balance read followed by a purse split would leave a race.

The native candidate lifecycle requires an important distinction between pre-state and post-user state.
It computes the funding plan from `current_root`, then restores retained `user_post_state` before `ApplyCostDeploy`.
Ordinary user execution can legitimately transfer funds or increase a purse balance between those two states.
A settlement guard must not require the post-user balance to equal the certified pre-state balance.
It must preserve the certified allocation and check whether post-user funds can satisfy that allocation.
An in-process top-up cannot increase the certified initial funding capacity or silently select a different fee payer.

The source review found that `SupplyReader` funding queries do not remain in the deploy event log.
Direct data reads occur outside the log, and temporary balance queries restore the earlier soft-checkpoint log.
Replay reconstructs admission from the same start root, but merge indexing reads the retained deploy log.
The later review corrected an overly strong inference from this observation.
Casper validates child certificates against their causal pre-states, not necessarily against one serial order across all branches.
A zero-debit purse read does not automatically require a new merge conflict when another branch changes that purse.
No regression currently demonstrates that the intended causal-prestate behavior is incorrect at this boundary.
This stronger serial-equivalence question does not block native fee integration.
The cursor's actual consume and update operations must remain in the committed lifecycle log because the cursor is shared mutable state.
It must not append query logs from discarded snapshots as if those queries belonged to the retained execution.

[`FeeCandidateSettlement.tla`](../../../../formal/tlaplus/cost_accounted_rho/FeeCandidateSettlement.tla) separates visible pre-state, each worker's retained user state, and atomic publication.
Its checked instance uses three payer positions, two workers, two transactions, and initial payer balances from zero through one.
An external purse starts with two units, and each candidate can transfer at most one unit from it into a payer purse.
The safe model and two negative controls passed.
The controls reject fee reselection after user execution and an unchanged-balance guard that prevents a funded top-up from committing.
The model proves an enabled commit for a fresh feasible candidate, not network liveness or distributed finality.
It still abstracts the native checkpoint and merge dependency mechanisms.

Cursor cells should remain separate for independent allocation scopes.
A single consumed cursor-map channel would couple otherwise independent cohorts.
Shared custody still needs atomic validation when different cohorts overlap.
No global execution lock or global state-root comparison is justified by this numeric allocator.

### Checked cursor transitions

The [cursor types](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/cursor.rs) now represent a revision, a position, and a scoped transition.
Their fields are private.
Constructors reject negative revisions, invalid positions, and malformed successors.
Both numeric fields use the signed 64-bit representation required by native Rholang integer datums.

Let $`v`$ denote the revision, $`s`$ the position, and $`n`$ the cohort size.
A readable cursor satisfies $`0 \le v \le 2^{63}-1`$ and $`0 \le s < n`$.
A settlement transition requires $`v < 2^{63}-1`$ and produces revision $`v+1`$.
The maximum revision remains readable, but another revision-changing settlement fails explicitly before allocation.
This representation limit is not a token supply limit or a forecast of node lifetime.

`MonetaryCohort::plan` validates the cursor before monetary allocation.
It derives the scope from the actual physical cohort and the supplied policy context.
It returns the monetary debit and the checked cursor transition together.
The plan also has private fields, so callers cannot independently replace either component inside the generated plan.
Native callers must supply an authenticated cohort and the trusted protocol policy context.
A standalone transition constructor does not authenticate an arbitrary scope hash.

### Zero new contribution

The approved contribution policy distinguishes zero new funding from the separate positive deployment fee.
[`MonetaryCohort::plan_all_to_all_contribution`](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/cohort.rs) implements this distinction for the capped all-to-all domain.
It does not classify arbitrary funding constraints as all-to-all.
The complete-domain classifier must establish that precondition before production use.

At zero obligation, the method validates the cohort and cursor and returns an empty settlement with no cursor transition.
It does not increment the revision or return an old cursor value for publication.
A valid maximum revision remains usable because this operation requires no successor revision.
An out-of-cohort cursor position still rejects the request.
Other authority, prepaid-backing, funding-consent, and execution checks remain necessary.

At positive obligation, the method delegates to the existing scoped planner.
The resulting debit, residual position, revision, scope, capacity errors, and overflow errors remain identical.
`MonetaryCohort::plan` retains its original behavior for existing callers, including the positive deployment-fee path.
The new method must not replace that fee with a zero contribution.

`AllToAllContributionPlan` has private fields and an optional cursor transition.
The absent transition means no contribution-cursor write.
Native publication must preserve this absence rather than serialize the captured cursor as a replacement value.
If another operation advances the cursor, publishing an old zero-result snapshot would erase that advancement.
The plan changes no state itself and introduces no lock.

[`ContributionCursor.v`](../../../../formal/rocq/cost_accounted_rho/theories/ContributionCursor.v) proves zero-write omission and exact reuse of the positive cursor check.
Its arbitrary-history theorem counts only successful positive contributions in the revision increase.
Further theorems preserve a positive successor across a subsequent zero operation and reject reuse of the earlier positive plan.
These proofs do not establish native atomic publication or the complete-domain classifier.

The [concurrent model](../../../../formal/tlaplus/cost_accounted_rho/ContributionCursor.tla) separates preparation, commit, and abort for two independent workers and two scopes.
It includes every initial cursor with two positions and revisions from zero through two.
Zero and positive plans can overlap on one scope or execute on separate scopes.
The model requires zero operations to preserve current state and positive operations to validate their captured cursor.
Negative controls detect both zero-result revision increments and stale snapshot restoration after a positive commit.

Native regression tests map these requirements to the planner:

| Requirement | Regression coverage |
| --- | --- |
| Zero obligation has no cursor write | Empty debit maps and absent transition, including the maximum revision. |
| Positive behavior remains unchanged | Exact comparison with the existing planner across capacities, cursor positions, and insufficient funding. |
| Cursor validation remains mandatory | An invalid position rejects even when no funding is required. |
| Physical aliases retain one allocation position | Generated cohorts through 129 purses compare logical and physical debits. |
| Revision counts only positive contributions | Generated mixed histories reach revision exhaustion while zero operations remain valid and unchanged. |
| Scope and stale-plan rejection remain intact | Positive plans reject a foreign scope and reject reuse after their successor. |

These tests exercise immutable planning and transition validation, not parallel RSpace publication.
The native checkpoint integration still needs its own concurrent publication and replay evidence.

### Positive transition validation

`checked_successor` checks the actual scope, revision, and position against the expected state.
It returns a proposed successor without changing storage.
The revision advances even when equal base allocation leaves the residual position unchanged.
Reusing the same plan after successful publication therefore fails, including after a complete rotation.
This property does not replace deploy identity checks or establish general transaction deduplication.

The focused [TLA+ model](../../../../formal/tlaplus/cost_accounted_rho/FeeCursorTransition.tla) represents two workers and two scopes with two positions each.
Initial revisions range from zero through two, including the exhausted revision.
Preparation and commit are separate actions, so both workers can prepare against the same cursor.
The model permits unchanged positions, changed expected positions, foreign target scopes, and aborts.
Its safe configuration and five negative controls passed before the Rust transition implementation.

The [Rocq development](../../../../formal/rocq/cost_accounted_rho/theories/FeeCursorTransition.v) proves six results with arbitrary natural-number bounds and history lengths.
The results cover scoped bounded successors, immediate reuse rejection, exhaustion, foreign scopes, revision counts, and stale plans after any positive history.
All six results compiled and passed independent kernel checking without additional axioms.
The proof treats scope equality abstractly and does not prove digest collision resistance or cryptographic authorization.

| Model requirement | Executable check |
| --- | --- |
| `ReceiptUsesExpectedRevision` | Exhaustive small cursor comparisons and rejection after unchanged positions or complete rotations. |
| `ReceiptUsesExpectedPosition` | Exhaustive comparisons include mismatched expected positions. |
| `ReceiptUsesExpectedScope` | Foreign scope rejection and a cohort-derived context change. |
| `RevisionCountsCommits` | Generated operation sequences count successful transitions independently and preserve state on discarded plans. |
| `CursorWithinRepresentation` | Negative-value, cohort-boundary, last-revision, and exhausted-revision cases. |
| Exact successor revision | Constructor tests reject skipped and reused revisions. |

All 29 allocation, cohort, and cursor tests passed after the plan-agent review.
The five focused Casper identity tests and strict Rholang/Casper library and test Clippy checks also passed on the reviewed changes.
The generated cohort property also compares scoped plans with numeric allocation across custody aliases, policy contexts, and revisions.
Another regression rejects a reduced cohort size when the expected position remains valid but the successor position does not.
The sequence property uses 256 cases with two prepared workers, three scopes, and independently generated cohort sizes from one through 65.
Operations include planning, retained-plan reuse, commit attempts, foreign-scope attempts, and discard.
These sequences exercise pure transition validation, not actual parallel RSpace execution.
They do not establish atomic initialization, coherent numeric-cell publication, monetary rollback, replay event retention, or branch merge behavior.
The native cursor checks below now cover some of those requirements.
The complete deployment and branch-merge requirements remain open.

### Native cursor storage and reads

[`SystemVault.rho`](../../../../casper/src/main/resources/SystemVault.rho) now stores each cursor in two private numeric channels.
The channel names combine the scope, field name, and a namespace created at genesis.
The directory contains an existence marker for each initialized scope.
An atomic `updateOrInsert` callback creates the initial pair only when the marker does not exist.
Existing scopes bypass directory mutation.
Independent scopes use different cursor channels, although initial directory inserts can contend on shared trie nodes.

The public read operation has this signature:

```text
SystemVault("costCursor", scope, returnChannel)
```

An absent cursor returns `[false, 0, 0]` without initialization.
An existing cursor returns `[true, revision, position]` through a joined peek of both numeric channels.
The [native decoder](../../../../casper/src/rust/util/rholang/costacc/monetary_cursor.rs) requires exactly one result with exactly three correctly typed fields.
It rejects malformed absence, extra results, negative values, and positions outside the expected cohort.
An exhausted revision remains readable.

Both `RuntimeManagerSupplyReader` and `RuntimeOpsSupplyReader` expose the checked read.
The latter rejects reads through an active replay runtime.
Replay must use a snapshot from the certified pre-state instead of adding exploratory query operations to its event log.
Ordinary funding preparation now reads this cursor before issuing its certificate.
Replay captures the cursor with its certified pre-state root, scope, and payer count.
It rejects evidence that does not match these bindings.

### Native settlement guard

`ApplyCostDeploy::with_fee_cursor` supplies a checked transition and payer count to the authenticated `SystemVault.applyCost` call.
The contract validates native integer bounds before initialization or monetary mutation.
It then consumes the actual revision and position together.
It compares both actual values with the expected values instead of waiting for literal expected values to arrive.
This distinction permits a stale request to fail without leaving an unmatched receive.

For a fresh request, the contract runs the existing reservation and monetary settlement operations while it holds the cursor pair.
Success publishes the successor pair.
Failure restores the actual pair, not the stale expected pair.
Each successful transition advances the revision, including transitions that leave the position unchanged.
The contract schedules result delivery and numeric publication concurrently, so a reply alone does not prove that both numeric datums are available.

The outer runtime checkpoint provides candidate rollback.
`play_system_deploy` restores its input root after explicit rejection or a platform error.
The internal evaluator alone does not provide this rollback boundary.
Ordinary candidate execution must retain the same money, cursor, and user-state rollback guarantees when integration is complete.
These checks do not establish recovery from a storage-write failure or process crash.

The contract accepts `Nil` cursor evidence only when every settlement entry has a zero monetary fee.
It rejects a positive fee without a cursor transition before monetary settlement.
Ordinary deployment certificates and both execution paths now supply mandatory validated evidence.
Zero-fee General burns and validator-fuel-only operations retain their existing path without a monetary cursor.
The extended native rejection test checks this guard against the complete input root and requires its exact rejection diagnostic.

### Native verification evidence

[`FeeCursorCells.tla`](../../../../formal/tlaplus/cost_accounted_rho/FeeCursorCells.tla) models two workers and two scopes with separate numeric publication steps.
Its invariants cover pair uniqueness, coherent acquisition, publication correspondence, and one revision per accepted fee.
The safe configuration and four negative controls passed before the native contract changes.
The controls remove initialization rechecking, revision validation, actual-value restoration, or position publication.
Each control violates its designated invariant.

This model uses initial revision zero and next positions zero or one.
It represents symbolic fees rather than purse balances.
It does not model checkpoint rollback, arbitrary revision histories, branch merge, or complete RSpace execution.
The separate cursor model and Rocq proofs cover bounded representation and arbitrary revision histories.
The candidate model covers abstract monetary conservation and abort preservation.
The native tests provide implementation evidence at these boundaries, not a proof that the models refine the complete node.

Four [native vault tests](../../../../casper/tests/util/rholang/monetary_cursor_vault.rs) passed after the reader additions.
The run completed in 69.54 seconds.
Strict Casper library and test Clippy checks also passed.

| Native check | Evidence established |
| --- | --- |
| Concurrent first use | Two authenticated calls with the same initial plan produce one accepted fee and one revision. |
| Stale reuse and independent scopes | Reuse fails without changing the root, while fresh transitions preserve unrelated scope state. |
| Failed initial charge | Insufficient funds restore the complete input root, including absent cursor state and unchanged balances. |
| Replay and snapshot reads | Initial and subsequent transitions produce equal play/replay roots. Both readers preserve state and distinguish absence. |

Three decoder tests also passed, including a 256-case property over presence, native bounds, and cohort sizes from one through 1,024.
The permanent results correspond to `fee-cursor-reader-unit.log`, `fee-cursor-reader-native-repaired.log`, and `fee-cursor-reader-clippy-repaired.log` in the verification directory.
The formal cell results are in `fee-cells-before-production.log`.

The expanded native suite then passed all six tests in 127.64 seconds, followed by strict Clippy.
The additional tests cover these boundaries:

- Ten malformed transitions preserve the entire root and all balances. Cases include negative values, cohort bounds, revision skips, and overflow.
- Successful settlement followed by explicit rejection restores the input root.
- Successful settlement followed by division by zero restores the input root.
- Successful settlement followed by a malformed result restores the input root.

Each post-charge failure runs against both an absent cursor and an existing cursor.
The test verifies the injected failure diagnostic, so an earlier failed charge cannot satisfy the rollback assertion accidentally.
The records are `fee-cursor-boundaries-reviewed.log` and `fee-cursor-boundaries-reviewed-clippy.log`.
These results cover the public system-deploy checkpoint boundary, not the complete ordinary deployment lifecycle.

### Exact reply validation

A regression found that the initial cursor decoder accepted a reply with an additional private name alongside its list.
The shared `RhoList`, `RhoBoolean`, and `RhoNumber` extractors can project values while ignoring some other `Par` content.
The failed test is `monetary_cursor_snapshot_rejects_parallel_names_in_response_and_fields`.
Its pre-fix result is in `fee-cursor-shape-before.log`.
This defect concerns the new reader, not an observed consensus failure or unauthorized vault mutation.

The decoder now reconstructs the canonical three-field reply and requires equality with the complete supplied value.
This check rejects extra content at the response level and within each field.
It also rejects list remainders and an unexpected `connective_used` flag.
The comparison uses the existing semantic `Par` equality, not protobuf-byte equality.
That equality intentionally ignores the transient `locally_free` analysis cache, as defined in `models/src/lib.rs` and documented in `models/build.rs`.
A separate regression preserves this existing cache behavior.
An initial test incorrectly required cache equality and was corrected after this source review.

[`FeeCursorReadBoundary.tla`](../../../../formal/tlaplus/cost_accounted_rho/FeeCursorReadBoundary.tla) checks this boundary before the decoder correction.
Its safe configuration passes, and its extra-content control violates `AcceptedContainsExactlyOneTypedReply`.
The model varies response count, field count, field typing, presence, numeric bounds, and extra content at all four projected locations.
It uses payer counts one through three and a maximum revision of three.
It treats extra content abstractly and does not implement the complete `Par` grammar.
This is a pure reply-validation model, not a concurrency model.
`FeeCursorCells` and the native concurrent-initialization test cover the separate storage synchronization boundary.

| Reply-model invariant | Regression or property |
| --- | --- |
| `AcceptedContainsExactlyOneTypedReply` | Parallel-name regression, metadata cases, and generated extra-content masks. |
| `ValidCanonicalReplyIsAccepted` | Generated valid replies and native reader tests. |
| `AbsenceHasZeroPayload` | Explicit malformed absence and generated numeric bounds. |
| `PresentCursorWithinBounds` | Negative fields, exhausted readable revisions, and generated cohort bounds. |

After the strict semantic comparison, the native reader and replay test passed in 41.62 seconds.
The result is in `fee-cursor-shape-native.log`.
All seven final decoder tests passed, including two 256-case properties.
Strict Casper library and test Clippy checks also passed on the final decoder.
The results are `fee-cursor-shape-final.log` and `fee-cursor-shape-final-clippy.log`.

### Optional cursor transport

The first expanded absence test failed before it reached the vault guard.
`ApplyCostDeploy::env` supplied bare `Nil` for an absent cursor.
The interpreter accepts an expression or unforgeable value at this injection boundary, but `Nil` contains neither.
`allocate_new_bindings` therefore returned `BugFoundError("invalid injection")`.
The result is in `fee-cursor-final-regression.log`.
Five other native cursor tests passed in that run.

This defect also affected valid zero-fee operations that omitted a cursor.
It originated in the new optional cursor argument, not the existing interpreter injection rules.
Changing interpreter rules is unnecessary.

The system-deploy environment now transports a one-element list containing the cursor value.
An absent cursor becomes `[Nil]`, while a present cursor becomes `[cursorTuple]`.
Both outer lists are valid injected expressions.
The system-deploy source requires exactly one element and passes that element to the unchanged vault argument.
Malformed transport produces an explicit error.
The wrapper does not create cursor authority or replace the authenticated SystemVault guard.

[`FeeCursorTransport.tla`](../../../../formal/tlaplus/cost_accounted_rho/FeeCursorTransport.tla) separates encoding, injection, delivery, and vault validation.
Its safe model and bare-`Nil` negative control passed before the transport correction.
The negative control violates `ValidInputCanReachVault` at the injection stage.
The model represents absent or valid cursor values and zero or one fee unit.
It abstracts cursor tuple contents, authentication, purse operations, and concurrent execution.
`ValidInputCanReachVault` excludes injection failure for modeled inputs. It does not prove scheduler fairness or eventual execution.

| Transport requirement | Implementation check |
| --- | --- |
| Valid input avoids injection failure | A 128-case property calls the actual `allocate_new_bindings` function with the emitted environment value. |
| Transport preserves the cursor | The property checks the absent value or every present tuple field after injection. |
| A missing cursor cannot pay a fee | The native test requires the specific vault rejection and an unchanged input root. |
| A zero fee remains valid | A native test checks burns of zero, one, and four units without cursor evidence. |
| Execution preserves replay | The zero-fee test compares native and replay roots and checks payer and recipient balances. |

The model result is in `fee-transport-before-production.log`.
All 12 vault-request unit tests passed after the correction, including the 128-case injection property.
All seven native cursor tests passed in 154.68 seconds.
These results include the exact missing-cursor rejection, zero-fee burns with replay, concurrent first use, and rollback checks.
The logs are `fee-transport-unit.log` and `fee-transport-native.log`.
The broader atomic-vault conservation and rollback regression also passed in 44.31 seconds.
Strict Casper library and test Clippy checks passed after the final correction.
The logs are `fee-transport-atomic-vault.log` and `fee-transport-clippy.log`.
These tests do not establish independent branch-merge correctness or full-node lifetime bounds.

### Independent branch-state merge

Shared-runtime concurrency and independent branch execution have different validation boundaries.
The shared-runtime test lets competing calls consume the same live cursor pair.
Independent branches can each start from a snapshot where that pair does not exist.
Their first-use event logs therefore contain no common pre-state cursor produce to consume.

The first [branch regression](../../../../casper/tests/util/rholang/monetary_cursor_branches.rs) tested only `are_conflicting` and failed its absent-cursor expectation.
The observed classification was:

| Pre-state cursor | Scope relation | Event-log conflict |
| --- | --- | --- |
| Absent | Different | No |
| Absent | Same | No |
| Present | Different | No |
| Present | Same | Yes, on two consumed produces |

This result does not establish a production safety defect.
The full merge path also checks numeric-cell overfill, including cells absent from the base state.
The plan-agent review found this guard in both the feature branch and local `dev` revision `cdf447ac18710d9702a27379bce6c946f421be46`.
The relevant implementations are `dag_merger::numeric_cell_would_overfill`, the overfilled-cell splitter, and `RholangMergingLogic::check_single_value_cell_not_overfilled`.
These mechanisms reject complete effect chains, not individual cursor fields.
The revision, position, directory update, payer debit, and recipient credit must remain in the same chain.
The cursor channels must remain outside additive numeric folding.

The expanded regression uses actual runtime roots, execution logs, mergeable-channel differences, and exact block effect indices.
It runs two runtime instances concurrently against the same pre-state.
The fixture supplies two sibling carriers and a common base to `dag_merger::merge`.
It tests absent and present cursors with shared and independent scopes.
Distinct payer and recipient purses isolate cursor conflicts from monetary conflicts.
Competing branches propose different successor positions to detect mixed cursor state.
The fixture does not test block signatures, finality, or the ordinary certificate-to-cohort authorization boundary.

[`FeeCursorBranchMerge.tla`](../../../../formal/tlaplus/cost_accounted_rho/FeeCursorBranchMerge.tla) models separate branch preparation and two independently interleaved validator merges.
Both validators receive the same two effects, pre-state, and canonical priority order.
The checked domain includes two scopes, all initial presence combinations, both priority orders, and all zero-or-one successor positions.
The safe model and all three negative controls passed.
The log is `fee-branch-model.log`.

| Model invariant | Native correspondence requirement |
| --- | --- |
| `CursorCellUnique` | The numeric-cell guard retains at most one writer per shared scope, including first use. |
| `WholeEffectAccounting` | Rejection removes the losing cursor changes and monetary effects together. |
| `IndependentScopesSurvive` | Disjoint scopes and purses retain both effects. |
| `SameScopeKeepsOne` | Competing fresh branch plans retain exactly one complete effect. |
| `ValidatorAgreement` | Reordered inputs produce identical merged roots and effect decisions. |
| `NoPhantomEffects` | Every retained effect belongs to the prepared input set. |

The controls remove empty-cell protection, retain rejected monetary effects, or permit different survivor ordering.
Each control violates its designated invariant.
The model treats monetary effects as symbolic per-effect charges rather than complete purse balances.
It assumes whole-effect field retention and abstracts the native priority calculation with an agreed total order.
It does not prove agreement on merge inputs, dependent-chain rejection, crash recovery, or general multi-parent consensus.
The native checks must establish the concrete effect grouping and state-fold correspondence separately.
No consensus implementation change is justified by the classifier-only result.

The expanded native regression passed all four scope/presence combinations in 113.85 seconds.
It checked reversed merge inputs, retained balances, cursor fields, and equality between rejected-branch merging and survivor-only merging.
The log is `fee-merge-state-correspondence.log`.
This result does not establish equivalence between direct execution and survivor-only merging.
That comparison exposed a separate [numeric accounting metadata defect](numeric-merge-authority-preservation.md).

## Verification and correspondence limits

| Artifact | Checked property | Current limit |
|---|---|---|
| [`RotatingMonetaryAllocation.tla`](../../../../formal/tlaplus/cost_accounted_rho/RotatingMonetaryAllocation.tla) | Exact total, capacity bounds, integer max-min fairness, valid cursor, zero-charge stability, and single-fee rotation. | Four positions, each capacity from zero through three, every funded amount, and every cursor. |
| [`RotatingFeeSettlement.tla`](../../../../formal/tlaplus/cost_accounted_rho/RotatingFeeSettlement.tla) | Concurrent preparation, fresh atomic settlement, one fee per transaction, receipt correspondence, abort preservation, and no duplicate settlement. | One abstract cohort, three positions, capacities zero through two, two workers, and two transactions. |
| Three unsafe settlement configurations | Stale-plan acceptance, lost cursor writes, and cursor advancement on abort each violate the named invariant. | These are model defects introduced deliberately, not three demonstrated production defects. |
| [`RotatingMonetaryAllocation.v`](../../../../formal/rocq/cost_accounted_rho/theories/RotatingMonetaryAllocation.v) | Seven arbitrary-list theorems cover payer count, residual conservation, no overdraw, one-unit spread, funded totals, and the water-level boundary. | The proofs do not establish Rust binary-search refinement, custody authentication, cursor persistence, or native concurrent settlement. |
| [Rust tests](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/tests.rs) | Exhaustive small allocations, independent unit-at-a-time oracle, configured-cap boundaries, rotation, saturation, maximum amounts, and generated invariants. | Tests do not prove arbitrary executions or native vault integration. |

Both safe TLA+ checks and all three negative controls passed before the numeric Rust implementation.
All eleven Rust tests passed, including four generated properties with 256 cases each.
The cap tests cover every configured cap from one through 64 and reject cap-plus-one cohorts.
The exhaustive allocation test covers all 256 four-position capacity vectors, every cursor, and every funded obligation.
The rotation test covers every initial cursor for each cohort size from one through 64.
An independent round-based oracle checks the complete output, including cursor behavior.
Additional tests cover cyclic reindexing, changing capacities, residual wraparound, and positive charges without residuals.
Strict Rholang library and test Clippy checks passed with warnings denied.

The plan agent found no blocking numerical defect during source review.
The review identified missing cursor cases, which the final focused tests now include.
The review also distinguished arbitrary serial settlement from canonical batch order and parallel-validator agreement.

All seven Rocq theorems compiled and passed an independent kernel check.
All seven theorems are closed under the global context.
The aggregate proof gate and the full integration suite have not been rerun for this change.

The transaction model represents parallel speculative workers within one settlement domain.
Any worker can commit next, so this model proves neither canonical batch order nor agreement about transaction-to-payer assignment across different schedules.
It does not represent parallel validators, several shards, deposits, crashes, storage writes, or multi-parent merge.
Those cases still require integration models and native tests before this repair can be declared complete.
The Rocq file proves list-order residual arithmetic but does not yet define cursor rotation.
Loom tests must exercise the actual synchronization implementation after that implementation exists.
A test-only lock around this abstract model would not establish native correctness.

The result logs are under `target/verification/claims-audit-20260909/`:

- `rotating-before-production-typed.log`
- `rotating-rocq-compile.log`
- `rotating-rocq-kernel.log`
- `monetary-allocation-reviewed-kernel.log`
- `monetary-allocation-reviewed-clippy.log`
- `fee-cohort-before-production.log`
- `fee-cohort-eligibility.log`
- `fee-cohort-physical.log`
- `fee-cohort-rocq-compile.log`
- `fee-cohort-rocq-kernel.log`
- `fee-cohort-native-identity.log`
- `fee-candidate-prestate.log`
- `fee-cohort-reviewed-rholang.log`
- `fee-cohort-reviewed-native.log`
- `fee-cohort-reviewed-clippy.log`
- `fee-rotation-native-before.log`
- `fee-cursor-before-production.log`
- `fee-cursor-rocq-compile.log`
- `fee-cursor-rocq-kernel.log`
- `fee-cursor-kernel.log`
- `fee-cursor-reviewed-kernel.log`
- `fee-cursor-reviewed-native.log`
- `fee-cursor-reviewed-clippy.log`

This permanent record preserves the results if build cleanup removes the logs.

## Native integration sequence

The plan agent reviewed the complete native path after approval of equal joint and individual participation.
The implementation connects all fee sites through the shared native planner.
Five calls in `acceptance.rs` now use the rotating monetary allocator.
They serve structural preparation, state-bound preparation, batch admission, incremental candidate settlement, and replay debit recomputation.
The runtime and replay then consume the certificate's fee allocation through `ApplyCostDeploy`.

| Component | Implemented integration |
| --- | --- |
| `FundingCertificate` and protobuf | Version 9 binds the fee plan, physical cohort, and logical allocation in the certificate hash. |
| `SupplyReader` and replay snapshot | Read the certified pre-state cursor. Only explicit absence supplies the initial cursor. |
| Reservation and admission | Plan one total fee. Advance the local cursor overlay only after candidate acceptance. |
| `ApplyCostDeploy` and `SystemVault.applyCost` | Require a cursor for positive fees, check its expected values, and publish the successor with successful settlement. |
| Runtime and replay | Recompute the fee plan from certified inputs and execute the same transition. Branch-merge coverage remains incomplete. |

Candidate extraction currently creates a provisional funding certificate before admission assigns its fee fields.
The native parser requires fee-plan evidence and rejects provisional certificates without it.
The generic Rust certificate retains an optional field for provisional construction and non-native resource-proof fixtures.
An absent wire field never becomes a zero cursor.
The cursor query must decode one complete pair or explicit absence from certified state.
Malformed responses must fail instead of losing unexpected values through filtering.

The fee plan binds nine fields:

- Policy version and policy context.
- Cohort scope and sorted physical custody identities.
- Monetary obligation.
- Expected revision and expected position.
- Successor revision and successor position.

The [checked evidence type](../../../../rholang/src/rust/interpreter/accounting/monetary_allocation/evidence.rs) validates shape, ordering, scope, and cursor transitions.
The [native planner](../../../../casper/src/rust/util/rholang/acceptance/monetary_fee.rs) additionally requires the native policy context and a one-unit fee.
That context is the Blake2b-256 digest of `f1r3node:monetary-fee:rotating-capped-max-min:v1:SystemVault:General`.
It identifies the flat native fee, not future phlo conversion or another asset.
Native replay reconstructs eligible physical purses, remaining capacity, and the cursor instead of trusting structurally valid evidence alone.

The native cohort cap uses the eligible signature count after existing host-work checks.
Physical custody deduplication can reduce that count.
This integration introduces no fixed two-payer limit and no separate new configuration parameter for purse count.
The revision must advance on each successful fee settlement, even when the residual cursor position does not change.
A position alone can repeat after rotation and cannot identify a unique cursor state.

Cursor initialization must be authenticated and atomic.
An absent-state query followed by an unconditional insertion does not provide that guarantee.
Independent cohorts must not share one consumed cursor cell.
Actual cursor consumption and publication must remain in the retained execution log.
Zero-debit balance reads do not independently justify a new merge conflict under the causal-prestate semantics described above.

The source review found no existing atomic missing-key update in `TreeHashMap`.
Its setter creates paths under consumed parent locks, but its updater returns without a callback when the path is absent.
The new [`updateOrInsert` operation](atomic-trie-upsert.md) combines checked path creation with a callback under the existing leaf lock.
Its two bounded safe models and four negative controls passed before the contract change.
The native fixed cases and generated concurrent updates now pass.
The operation supplies atomic first-use initialization for the native cursor guard.
It does not by itself prove settlement or branch-merge correctness.

The new SystemVault directory stores existence markers, not per-deploy cursor addresses.
Separate revision and position channels derive from the scope and one genesis-private namespace.
Both channels contain untagged numeric datums and remain outside additive merge handling.
This representation avoids packed counter arithmetic and gives concurrent initializers the same cursor addresses.
The existing numeric-cell merge guard is a candidate dependency, not proof that this design is safe.
The native checks above cover direct concurrent initialization, distinct scopes, logged system replay, and system-deploy rollback.
Same-scope branch merge and ordinary deployment failure paths still require further integration checks.

The [activation design](deploy-envelope-v6-1.md#activation-and-migration) requires fresh genesis.
It does not authorize an in-place upgrade of historical state.
Casper protocol version 6 and authority-accounting certificate version 9 identify different formats.
The certificate and witness hash domains now use version 9 for mandatory monetary fee evidence.
The native parser rejects version-8 evidence instead of reinterpreting it.
An existing-history replay requirement would require a separate compatibility decision.

### Client certificate compatibility

The September 10 authority test run exposed a missed client-format update.
The Rust certificate used the version-nine domain, protocol value, and optional fee-plan encoding.
Its golden assertion still used the version-eight digest.
The Python client in `../pyf1r3fly-d3-ci/` also still accepted version eight only.
This was a cross-repository integration omission, not an authority-valuation or Casper defect.

An independent Python encoder using `struct.pack` and `hashlib.blake2b` reproduced the old digest before the fixture changed.
It then calculated both version-nine encodings from explicit fields.

| Fixture | Encoded bytes | Blake2b-256 digest |
| --- | --- | --- |
| Version eight, historical fixture | 402 | `1a6cbf75519760b1729bf5a5f0c876c6a2af8e7bf492cbffb40f27d5dd060eef` |
| Version nine, internal certificate without a fee plan | 403 | `093145bb99125f8918e7c93f711a6164c7f8cfc6d166a9f4657b1c8a19410205` |
| Version nine, native fee-plan fixture | 583 | `8baf872246032dda3ce22fa51ad22371420acd16256b6b593e68492e58c71257` |

The common fixture uses program, pre-state, and reservation identities filled with bytes `m`, `p`, and `r`, respectively.
Each identity contains 32 bytes.
Exact demand and balance allocation each contain two units under the 32-byte `s` key.
The 32-byte `k` stack reserves one pop, and the 32-byte `g` fee key contributes one unit to recipient `proposer`.
Byte accounting uses schedule version one, its existing digest, and zero bound and allocation.

The native fee fixture uses the approved SystemVault policy context and the ordered 32-byte `g` and `h` payer identities.
Its obligation is one unit, and its cursor changes from revision/position `(0, 0)` to `(1, 1)`.
Rust validates this evidence before hashing it.
The Python client independently validates and hashes the same fixture.
The absent-plan fixture remains an internal encoding check, not evidence accepted by native admission or the client.

The client protobuf now includes `CostMonetaryFeePlanProto` and certificate field 17, `feePlan`.
The parser requires that plan and validates the native policy, scope, ordered custody identities, exact fee, and bounded cursor successor.
It includes every canonical fee-plan field in the certificate hash.
Missing plans and old versions fail explicitly, consistent with the documented fresh-genesis boundary.
The client does not authenticate custody or recompute balances and allocations from node state.

Before the repair, `FeeCursorTransition.v` passed fresh compilation and independent kernel checking under a 2 GiB, no-swap systemd scope.
All six reported theorem assumptions were closed under the global context.
The proof digest is `46e0be464bc77a14051eaaa1dc25ed4865f424a65fe81291e6c353aba25826d3`.
These proofs cover scoped bounded successors and reuse rejection, not Python extraction or cryptographic collision resistance.

Client regression tests check the shared digest, malformed fields, changed valid plans, and the same cursor bounds.
Encoding tests cover every cohort size from one through 129, three revision boundaries, and three position pairs.
Each case compares canonical bytes with a separate `struct.pack` oracle and checks protobuf round-trip equality.
Reusing a completed transition fails validation unless a new successor is supplied.
The range is a test bound, not a protocol restriction on the number of wallets.

The local client suite passed 301 tests, excluding live-node integration tests.
Client mypy checked 56 source files without errors, and the repository isort check passed.
The Rust authority suite passed 57 tests, and strict Rholang library/test Clippy passed.
The client source digest is `176d8f6e6b6cc00b7aee3196edd28a49b23e0841db3923d8818778a6a9b5a93d`.
The client test digest is `b3fa82e30a2daf8e56a8883b1c1d144c3b88a14729cf0d9319f6e26cb107b5ef`.
No new node wire behavior or Casper rule changed in this compatibility repair.
The repair updates the client and tests to the already documented node format.

### Native fee integration evidence

The native regression uses three selected signers and their funded joint purse across four committed roots.
It checks one recipient fee per deploy, native/replay root equality, actual purse losses, and equal cumulative fee allocation.
The unchanged production path completed all four deployments and replay checks in 114.37 seconds.
The final assertion failed with cumulative fees `[0, 0, 0, 4]`, instead of `[1, 1, 1, 1]`.
The funded joint purse paid every fee.
This result demonstrates the difference from the newly approved allocation policy, not a replay disagreement.
It is separate from the existing leaf-only regression that detects multiplied monetary fees.
This pre-integration result did not inspect cursor evidence.
The repaired regression checks all four cursor revisions, positions, ordered custody identities, and one-unit obligations.
Both native multi-payer tests passed in 285.65 seconds after integration.
The second test covers empty and communicating programs with one, two, and three signers, including a two-of-three selection.
Their result is in `fee-integration-native.log`.

All 31 allocator and evidence tests passed, as did eight replay-payload binding tests.
The binding tests mutate every fee-plan protobuf field and require a different replay cache key.
Strict Rholang and Casper library and test Clippy checks passed.
The acceptance suite initially passed 82 tests and failed one expected-error assertion.
That test removed the fee, which the new policy decoder rejects before canonical replay allocation.
The revised test checks a valid baseline, missing fees, doubled fees, and the maximum unsigned amount.
An additional generated property rejects incorrect fee totals at the wire decoder.
All 84 acceptance tests passed after these changes in 28.35 seconds.
The result is in `fee-acceptance-regression.log`.
These focused results do not establish full CI success or branch-merge safety.

## Remaining integration requirements

The allocation choice settles rotation, but it does not specify every economic or activation rule.
The integration review identified these decisions:

1. Preserve one position per physical purse without changing logical authority multiplicity.
2. Bind the cursor scope to the approved physical cohort and native policy context.
3. Apply the approved equal monetary participation of authorized joint and individual purses.
4. Define cursor initialization and behavior for charged deterministic user failures.
5. Apply fresh-genesis activation and an explicit accounting-certificate version. Do not invent an installed-contract migration.
6. Specify whether authenticated block evidence satisfies the planned signed-output requirement.

These decisions must not silently restore lexical bias or change monetary policy through a helper function.
The [funding path audit](../multi-wallet-funding-path-audit.md) records the demonstrated bugs and the existing economic distinctions.
