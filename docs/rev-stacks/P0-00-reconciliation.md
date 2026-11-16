# P0-00 — Reconciliation of Track A with current dev

Patch P0-00 of the rev-stacks plan (stall-loop design r6, §8.1–8.2, plan
`rev-stacks-patch-plan-r6.3`). Written list only, no code. For review before
P0-05, P0-10, P0-11 and P0-13 start.

- **Base:** `dev` @ `252a86d1d` (tag `rev-stacks/base`). `fix/stall-track-a-on-dev`
  and `patch/P0-00` both exist and sit at that commit, so this list is written
  against plain dev.
- **Track A sources:** `fix/finality-stall-track-a` @ `6deb949b6`,
  `fix/finalizer-forward-progress` @ `c36932d26`, `fix/stall-track-a-rest` @ `0cb21310d`.
- **Method:** each Track A commit was read against the dev code that replaces
  it, and trial cherry-picked (`git cherry-pick -x --no-commit`) onto
  `252a86d1d` in a scratch worktree. The conflicts it reports are listed below.
- **Design and audit:** `F1R3FLY-io/publications` @ `315b815`,
  `cost-accounting/stall-loop-design-r6` and `f1r3node-profiling/clone-audit.tex`.

## Summary

| Item | Track A commit(s) | dev counterpart | Verdict | Plan patch |
|---|---|---|---|---|
| A1a | `8ba8b77f1` | `2edcb0bcc`, finalizer task `3a455a679`, `d03621406` | **Reduce**: keep the cross-run oracle fact cache and the bounded rerun after a timeout | P0-05 |
| A4 | `600689764`, `c4b8b40c4` | `30453da91`, `23dc2cd53`, `f1bc683d5` | **Reduce**: keep verify-once decoding, one rejected-buffer read per proposal, and the pool cap under the admission guard | P0-10 |
| A5 | `1a928edc4` | `ec3e3ef4b`, `c4f0a1815` (also `c9aff7732`), **not** `bbfd6c00d` | **Drop**: dev's settle already makes one walk for all open rows, and keeps a memo across cycles | P0-11 → drop |
| A7 | `46d49cb83` | `a596e3eea`, `ecef0dd68`, `2051b551a` | **Drop** (confirms the plan) | — |
| Clone audit, Tier 0 | — | 0.1 `3e5ccbc15`, 0.2 `708a5451f`, 0.3 `d99302d21`, 0.4 `fff7021cd`, 0.5 `e48499e2f` | dev has 0.1–0.5. **0.6 and 0.7 are still open.** | P0-13 → re-scope or drop |

Two corrections to the plan come out of this list (see "Points for review"):
P0-11's conflict comes from `ec3e3ef4b` and `c4f0a1815`, not from `bbfd6c00d`,
and P0-13 as written (0.1, 0.2, 0.3, 0.5) has nothing left to port.

---

## A1a — Oracle progress across timed-out runs

**Track A, `8ba8b77f1` (12 files, +186/−19), does two things:**

1. **A shared oracle fact cache that outlives one run.** `CliqueOracle::new_shared_cache()`
   creates a `CliqueOracleRunCache` behind a `tokio::sync::Mutex`, raises its
   bounds to 200,000 entries, and keeps it on `MultiParentCasperImpl`
   (`finalizer_oracle_cache`). An `OracleCacheMeter` (a `WorkMeter` with a new
   `oracle_cache()` hook in `shared/.../observation_work.rs`) carries the cache
   into `floor_of_view_metered`, and `ft_witnessed_exact_metered` uses it in
   place of a fresh run cache. **For correctness, `latest_justifications_cache`
   is re-keyed from validator `V` to message `M`.** Keyed by validator, it is
   only valid within one snapshot. Keyed by message, all three caches
   (justifications of a message, its self-justification, ancestry pairs) hold
   facts about immutable blocks, so they stay valid across snapshots and runs.
2. **An immediate rerun after a timeout.** `drive_finalizer_runs` reruns up to
   `MAX_TIMEOUT_RERUNS = 3` consecutive timed-out cycles with no new trigger, so
   the work cached by the run that timed out is resumed at once.

**What dev already does:**

- `2edcb0bcc` adds `WitnessMemo` (`casper/src/rust/finality/floor.rs`). It
  memoizes oracle **verdicts** and **committees** by target, for **one
  derivation over one snapshot**. Its own comment says it must never be reused
  across snapshots. The commit also writes the validated floor and frontier to
  their caches (`validation_dispatcher.rs`, `floor_context.rs`).
- Each oracle evaluation still calls `CliqueOracle::new_run_cache()`
  (`clique_oracle.rs:793`, and `:638`). The low-level fact caches therefore
  start empty on every call, and `latest_justifications_cache` is still keyed
  by `V` (`clique_oracle.rs:102`).
- `3a455a679` and `d03621406` replaced `drive_finalizer_runs` with a single
  finalizer task (`finalizer_loop`) and `run_cycle`. The loop reruns only when
  `requested` rose during the cycle. A cycle that times out with no new trigger
  waits for the next trigger. `run_cycle` logs "skipping this cycle" and does
  not report a timeout to the loop.

The two caches sit at different levels and compose. `WitnessMemo` avoids
repeat oracle calls within one derivation. Track A's cache makes each oracle
call cheaper across derivations, snapshots and timed-out runs. dev has no
counterpart to either Track A part.

**Keep:**

- The shared fact cache, including the re-key of `latest_justifications_cache`
  to `M`, which is required. Also keep the `oracle_cache()` hook on `WorkMeter`
  and `OracleCacheMeter`. Have `compute_last_finalized_block` call
  `floor_of_view_metered(&meter, …)` in place of `floor_of_view`.
- The rerun after a timeout, capped at 3. Re-implement it in dev's loop: have
  `run_cycle` return whether it timed out, and have `finalizer_loop` treat a
  timeout below the cap as a request (bump `requested` or re-notify `wake`).
  Keep the reset-on-success behavior (Track A test
  `a_run_that_finishes_after_a_timeout_stops_resuming`).
- Tests: `a_timed_out_run_resumes_without_a_trigger_up_to_the_cap` and
  `a_run_that_finishes_after_a_timeout_stops_resuming`, rewritten against
  `finalizer_loop`/`run_cycle`. Also
  `a_shared_oracle_cache_gives_the_same_frontiers_across_snapshots`, with the
  `&mut WitnessMemo::default()` argument that dev's `cold_parent_frontier_metered`
  now takes.

**Reduce or change:**

- **Where the cache lives.** Hold it in `finalizer_cycle(ctx)` beside dev's
  `LineageMemo`, or on `FinalizationContext` built there, not on
  `MultiParentCasperImpl`. dev's finalizer is one task per Casper instance and
  the only consumer. This removes the conflicts in `casper.rs`, `types.rs` and
  the six test files (`bonded_status_api_test`, `deploy_finalization_status_test`,
  `last_finalized_api_test`, `pending_deploys_test`,
  `multi_parent_casper_bonding_spec`, `test_node.rs`). In those files the only
  change was the new struct field.
- **Bound.** At 200,000 entries, `latest_justifications_cache` holds one
  `BTreeMap<V, M>` per message. That is about 200,000 × V × ~100 B, which is
  hundreds of MB at tens of validators. Eviction removes the smallest hash, so
  it is effectively random. Size the bound by measurement or as a multiple of
  the validator count (see review point 3).

**Drop:** nothing else. The `drive_finalizer_runs` hunks do not apply because
that function no longer exists. The behavior is re-implemented as above.

**Trial cherry-pick:** conflicts in 9 files: `finalization_runner.rs`
(5 hunks), `casper.rs`, `types.rs` and the six test files above (1 hunk each).
The reduction above removes 8 of the 9. The `finalization_runner.rs` change is
a re-implementation of logic in dev's new loop, not a signature change. P0-05
should treat it as new code reviewed against `8ba8b77f1`, not as a mechanical
port.

**Ordering note:** A1b (P0-06, `5d78f252c`) is still needed. dev's `run_cycle`
times `compute_last_finalized_block`, and that includes `adopt` (the commit).
P0-05 should keep the timeout signal so that P0-06 can narrow what it covers.

---

## A4 — One pool read per proposal

**Track A, `600689764` (4 files, +196/−42):**

1. `PoolSnapshot` (`block_creator.rs`) reads the deploy pool **and the
   rejected-deploy buffer** once per proposal (`stored`, `buffered`,
   `buffered_sigs`). It passes them to
   `storage_has_unresolved_in_scope_deploys_in`, `fresh_local_deploy_stats_in`,
   `in_scope_local_deploy_stats_in` and `rejected_buffer_has_recoverable_deploys_in`.
2. **Verify-once decoding** (`block-storage/.../deploy_codec.rs`,
   `models/.../casper_message.rs`). A process-wide set
   `VERIFIED_ENCODINGS` (cap 262,144, cleared when full) holds the Blake2b-256
   hash of each stored payload whose signature has been verified. A later
   decode of the same bytes calls a new `DeployData::from_stored_proto`, which
   skips signature verification. Tampered bytes hash differently and are still
   rejected (test `a_verified_encoding_is_trusted_on_later_reads_and_nothing_else_is`).
3. **Pool cap under the insert lock** (`block_admission.rs`). `reserve_deploy`
   returns `Reservation::{Added, Duplicate, PoolFull}` and checks
   `pool_admission(pending_count, cap)` in the same critical section as the
   insert. A3's earlier check sat outside it, so concurrent admissions could
   overshoot the cap.

**Track A, `c4b8b40c4` (5 files):** two `PoolSnapshot::read` unit tests, plus
unrelated drive-bys: a `filter_map_bool_then` clippy fix in
`block_dag_key_value_storage.rs` (code from A8, `5d2f3aae2`), `#[allow(dead_code)]`
on `ALinPolicy` (introduced by `a6f9155bb`, A1c part 2), and
`Cargo.lock`/`deny.toml` supply-chain updates.

**What dev already does:**

- `30453da91` and `23dc2cd53` (issue #711) read the pool once per proposal.
  `create_with_progress` takes one `deploy_storage.read_all()`
  (`block_creator.rs:2921`) and passes `&stored_deploys` to the scope check and
  both stats functions. The scope check uses `any`. This covers Track A part 1
  for the **pool**.
- **The rejected buffer is still read up to four times per proposal.** On
  dev, `fresh_local_deploy_stats` (`:2141`), `in_scope_local_deploy_stats`
  (`:2209`), `rejected_buffer_has_recoverable_deploys` (`:2291`) and
  `prepare_user_deploys_with_policy` (`:610`) each lock it and call
  `read_all()`. The #711 plan kept "buffer reads … in their current paths" on
  purpose.
- `f1bc683d5` (issue #712) removed the pool mutexes. Per
  `docs/claims/deploy-store-concurrency.md` S4, **the rejected-buffer mutex
  still covers the buffer check and the atomic pool insert**, so admission is
  still serialized on that guard. `reserve_deploy` on dev has no pool lock.
  It holds the rejected-buffer guard and calls `deploy_storage.add_if_absent`.
- dev's `deploy_codec::decode` still calls `DeployData::from_proto`, which
  verifies the signature on every read. `f1bc683d5` changed only its tests.
  There is no verify-once cache and no `from_stored_proto`.
- dev has no pool cap at all. `deploy_pool_cap`, `pool_admission` and
  `pending_count` come from A3 (`d1c3a970f`, P0-09).

**Keep:**

- **Verify-once decoding** (`deploy_codec.rs`, `from_stored_proto`, and its
  test). With dev's single pool read, each proposal still verifies a signature
  for every pooled deploy (plus every buffered one, up to four times). Verify-once
  makes this O(new deploys). The design names it in §3.2 ("about 8N signature
  checks per proposal"). It applies without conflict: dev's `decode` is
  unchanged.
- **One rejected-buffer read per proposal.** Port only the `buffered` and
  `buffered_sigs` half of `PoolSnapshot`. Read the buffer once in
  `create_with_progress` next to dev's `stored_deploys`, and pass it to the
  three stats/recovery functions and to `prepare_user_deploys_with_policy`.
  Keep dev's empty-buffer shortcut (`non_empty()` before `read_all`).
- **Pool cap in the admission critical section.** Re-express it on dev with no
  pool lock: check `pool_admission(pending_count, cap)` **while holding the
  rejected-buffer guard** in `reserve_deploy`, right before `add_if_absent`.
  By S4 that guard serializes admissions, so the cap is exact against
  concurrent inserts and conservative against concurrent removals. Keep
  `Reservation` and the `pool_full` lifecycle event. This needs P0-09 (A3)
  first, as the plan says.
- Tests: the two `PoolSnapshot::read` tests from `c4b8b40c4`, adapted to the
  buffer-only snapshot. Add a test that the cap holds under concurrent
  `reserve_deploy` calls.

**Drop:**

- The **pool** half of `PoolSnapshot`, the `_in` split of the scope and stats
  functions, and the `#[cfg(test)]` wrappers. dev's #711 work has these.
- `PoolSnapshot::read` taking `deploy_storage.lock()`. That mutex is gone on dev.
- From `c4b8b40c4`: the `Cargo.lock`/`deny.toml` hunks (dev carries its own
  supply-chain state). The `block_dag_key_value_storage.rs` clippy fix belongs
  with P0-02 (A8) if the lint fires there. The `ALinPolicy` allow belongs with
  P0-07 (A1c part 2).

**Trial cherry-pick:** `600689764` has 9 hunks in `block_creator.rs` and 1 in
`block_admission.rs`. `c4b8b40c4` has conflicts in `block_dag_key_value_storage.rs`,
`block_creator.rs`, `finalization_runner.rs` and `deny.toml`. Almost all of
them are in the dropped half. The kept parts are small.

---

## A5 — Batch the lifecycle register walk

**Track A, `1a928edc4` (1 file, `deploy_lifecycle.rs`):** replaces the
per-sig `effect_in_state_of_above` walk (which decoded full blocks) with
`lineage_membership`. That is one walk of the max floor's lineage over cached
`block_facts`, answering every due sig's `MembershipProbe` (sig, `min_height`,
`checked_below` memo). The commit adds a generated-lineage equivalence test.

**What dev already does:** dev rewrote the register.

- `ec3e3ef4b` ("Settle deploy verdicts in the finalizer, the one writer of the
  LFB") removed `effect_in_state_of_above` and the `evaluate` loop that A5
  patched. `settle` reads the open rows and makes **one** `walk_lineage` of the
  LFB's lineage over `block_facts`, down to the lowest open `valid_after`, for
  all open rows (`SettlePass::walk`). It records `applied_at` per sig.
- `c4f0a1815` adds `LineageMemo`, held across finalizer cycles in
  `finalizer_cycle`. A later settle stops at the previous walk's start and
  reuses its answers below it. Track A had no cross-cycle reuse.
- `c9aff7732` (August) already batched the merge's settled-sig probes
  (`settled_sigs_of_lineage`, `FloorSettledProbe`).

**`bbfd6c00d` is not A5's counterpart.** It changes the **proposer's**
unfinalized-cone walk during deploy admission (`block_creator.rs`, plus a
`timestamp` field in `BlockFacts`). That covers the stall finding "Unfinalized
cone walked once per parent with full block decodes". The trial cherry-pick of
`1a928edc4` conflicts only in `deploy_lifecycle.rs` (4 hunks), against
`ec3e3ef4b` and `c4f0a1815`. It does not touch `block_creator.rs`.

**Verdict: drop A5 entirely.** Everything it added exists on dev, in a stronger
form: one walk for all rows, over cached facts, with a memo across cycles. The
function it modified no longer exists. Its equivalence test is covered by
dev's `reference_effect_in_state` checks in `deploy_lifecycle.rs`.

**Residual, outside A5:** dev still walks per sig in the proposer
(`block_creator.rs:3198`, once per selected deploy, over cached facts) and in
`assert_no_settled_rejection` (`interpreter_util.rs:2223`, rejected records ×
settled floors). Both stop early and read cached facts. They are not part of
Track A and are noted here only so the "observe time flat in open signatures"
goal is not credited to them.

---

## A7 — Buffer eviction and re-fetch cooldown

**Track A, `46d49cb83`:** evicts the oldest dependency-free pendant first,
sorting once (`casper_buffer_key_value_storage.rs`). A duplicate copy of a
block still in the buffer calls a new `BlockRetriever::ack_duplicate`, which
marks the request `received` without clearing the cooldown.

**What dev already does:**

- `a596e3eea` (TASK-021-18, explicitly "stall-loop item A7") does the same
  oldest-first eviction with a single sort (`casper_buffer_key_value_storage.rs:479`).
  It drops a duplicate copy of a buffered block **without any ack**, so the
  re-fetch cooldown, retry counter and quarantine stay
  (`block_processor.rs:837–845`). Tests:
  `the_overflow_prune_removes_the_oldest_dependency_free_entry_first`,
  `refetch_cooldown_spec`.
- `ecef0dd68` and `2051b551a` go further. Evicted pending blocks stay
  retrievable: every DAG insert persists the body, an eviction re-requests the
  hash through `recover_dependency` so that a `received` request state cannot
  suppress the re-request, a refusal gets its own outcome, and so on.

**Verdict: drop**, as the plan says. The one behavioral difference is that
Track A marks a duplicate's request `received` and dev leaves it unchanged.
Under `2051b551a`, a `received` state is something dev deliberately routes
around, so porting `ack_duplicate` would work against dev's recovery path.
Track A's `block_retriever_spec` addition tests `ack_duplicate` and goes with
it. Trial cherry-pick: 1 hunk in `casper_buffer_key_value_storage.rs`.

---

## Clone-audit Tier 0 on dev @ 252a86d1d

The audit was written against dev @ `e8008c3` (Oct 20). Each item was checked
at the cited location on `252a86d1d`.

| # | Defect | On dev? | Evidence |
|---|---|---|---|
| 0.1 | Stream reassembly quadratic (`comm/.../stream_handler.rs`) | **Fixed** | `3e5ccbc15` "append received stream chunks in place": `cache.entry(key).or_default().extend_from_slice`. `restore` takes the buffer with `cache.remove`. |
| 0.2 | Replay COMM scan with a clone per step (`rspace++/.../replay_rspace.rs`) | **Fixed** | `708a5451f`: `get_comm_or_candidate` borrows `&[COMM]` and uses `iter().find_map`. `matches` and `was_repeated_enough_times` take `&COMM`. |
| 0.3 | `Env::put` copies the whole environment (`rho-pure-eval/src/env.rs`) | **Fixed** (both steps) | `d99302d21` "Keep the evaluation environment persistent": `imbl::Vector` bindings, `put(mut self)`, and `shift` is a pointer bump. |
| 0.4 | `EventLogIndex::combine` in folds | **Fixed** | `fff7021cd`: `combine_all` and in-place fold accumulators in `deploy_chain_index.rs` and `dag_merger.rs`. |
| 0.5 | `eval_match` copies the remaining cases | **Fixed** | `e48499e2f` "Let evaluation own the terms it consumes": `for case in mat.cases`, by value. |
| 0.6 | `SortedParMap::extend` re-sorts once per element. `SortedParHashSet` has the same shape. | **Open** | `models/src/rust/sorted_par_map.rs`: `extend` still calls `insert` in a loop, and each call clones and rebuilds. Medium risk (canonical form). It needs the sorter differential guard. |
| 0.7 | LFS requester state copied per block (`casper/.../lfs_block_requester.rs`) | **Open** | Still clones `d`, `latest`, `height_map`, `finished`, `mergeable_d` per `add`/`done` (lines 136–205). Affects initial sync only. |

**Consequence for P0-13:** its scope (0.1, 0.2, 0.3, 0.5) is already on dev.
`fix/clone-audit-tier0` is not needed for those. Either drop P0-13 or re-scope
it to 0.6 (with the differential test) and 0.7 (see review point 2).

---

## Points for review

Resolved under the revised operating rules. See `decisions/P0-00.md`
(D-P0-00-1 to D-P0-00-5). Original points, for the record:

1. **P0-11 → drop.** Mark A5 dropped, like A7, citing dev `ec3e3ef4b` and
   `c4f0a1815`. Correct the plan's conflict note from `bbfd6c00d` to those two
   commits.
2. **P0-13 → decide:** drop it (Decision 13 then holds trivially), or re-scope
   it to 0.6 and 0.7. Of the two, 0.6 is the one on a user-visible path (map
   and set `+`/`++` in Rholang). 0.7 affects only initial sync.
3. **A1a cache bound:** confirm 200,000 entries, or ask P0-05 to set the bound
   from a measurement or as a multiple of the validator count.
4. **A1a placement:** confirm the cache lives with the finalizer task
   (beside `LineageMemo`), not on `MultiParentCasperImpl`. This keeps P0-05
   out of `casper.rs`, `types.rs` and the test harness, which other lanes touch.
5. **A4 cap placement:** confirm the cap check goes under the rejected-buffer
   guard (dev's admission serialisation point, claim S4), since dev has no pool
   mutex to hold.

## Not ported, and why (for the Report-back record)

| Commit | Reason |
|---|---|
| `1a928edc4` (A5) | Superseded by dev `ec3e3ef4b`, `c4f0a1815`. |
| `46d49cb83` (A7) | Superseded by dev `a596e3eea`, `ecef0dd68`, `2051b551a`. |
| `600689764`, pool half of `PoolSnapshot` | Superseded by dev `30453da91`, `23dc2cd53`. |
| `c4b8b40c4`, `Cargo.lock`/`deny.toml` | dev carries its own supply-chain state (as for `f2e8fb896`, `05f2a3fa7`, `155153ed4`, `435e47554`). |
| `8ba8b77f1`, `drive_finalizer_runs` hunks | Function removed by dev `3a455a679`. The behavior is re-implemented in `finalizer_loop` (P0-05). |
