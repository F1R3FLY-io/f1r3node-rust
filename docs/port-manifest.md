# Port manifest — `feat/floor-sealed-merge` (PR #77) → `feat/sealed-floor-merge-v2`

Per-commit inventory of what to carry from the reference branch into the clean v2 rebuild.

**Source (reference):** `feat/floor-sealed-merge` — [PR #77](https://github.com/F1R3FLY-io/f1r3node-rust/pull/77) (consensus work = commits 26-79 + 2 new).
**Target:** `sealed-floor-merge-wip` (off `feat/sealed-floor-merge-v2`).
**Green-gate:** `casper/tests/batch2/map_cell_convergence_spec.rs`.
**Companion narrative:** [`docs/sealed-floor-merge-v2-status.md`](./sealed-floor-merge-v2-status.md).

## Current v2 status

All applicable PRESERVE / REBUILD items below have been ported or adapted on
`sealed-floor-merge-wip`. The only deliberate non-port is `0bb91b22`'s direct endpoint hunk:
on the reference branch `exploratory_deploy(None)` meant FS(LFB), but in v2 it still means a
speculative merge over current DAG tips; the v2 web endpoints already resolve omitted block hashes
to the LFB and pass that explicit hash.

The manifest remains as provenance for review. Rows that name unavailable historical commits
without corresponding fetched objects are treated as notes, not unported work.

## Verdict legend
- **PRESERVE** — independent keeper; cherry-pick or port ~verbatim.
- **REBUILD** — the new design implements this concept fresh; the branch commit is *reference*, not a cherry-pick.
- **DROP** — regressed / superseded / scaffolding; do not bring.
- **IN-STAGING** — already in staging (don't re-port).
- **MIXED** — the commit splits; the diff must be split at port time (flagged).

> ⚠ Several entries are MIXED — a single commit did a keeper AND a redesign/scaffolding thing in the same diff. Those require hunk-level splitting when ported; do NOT cherry-pick the whole commit. Re-read the diff for every MIXED row before porting.

## A. PRESERVE — independent keepers (carry over regardless of the redesign)

| commit | what | port note |
|---|---|---|
| `3499b39e` (part) | **remove the bonds-equality parent filter** | stop orphaning differently-bonded siblings; small, isolate from the seal parts of this commit |
| `3499b39e` (part) | **fold system-deploy chains into conflict detection** | makes concurrent CloseBlock PoS-cell writes visible to conflict detection; verify still needed under new merge |
| `85a75ffa` | **on-demand mergeable-entry recompute** (`ensure_scope_mergeable_present`) + **ancestry-based `repeat_deploy`** | load-bearing for LFS-imported / cross-node determinism. repeat_deploy may shift to record-based — keep the ancestry direction |
| `f0decf13` | **full-bonds finality denominator** (revert active-set weighting) | SAFETY fix — active-set denominator let a pause shrink quorum, finalizing under FTT. Keep full-bonds denominator |
| `9d1f5d1d` | **total-order tiebreak in optimal rejection** | kills the HashSet coin-flip; node-deterministic merge rejection |
| `16b2e980` | **reject multi-value IntegerAdd (not MAX-fold)** | CRITICAL — loud error instead of silent concurrent-write loss; keep `fold_bitmask_or` |
| `22299115` | **LMD-GHOST main-parent selection** (`estimator.tips_with_latest_messages`) | the revived fork-choice. parents[0] = stake-heaviest convergent tip |
| `33`/`39`/`82dfb222`/`ec2cb29f` | **DAG-ancestry helpers + floor-derivation soundness** (`is_general_ancestor`, `is_dag_ancestor`, co-finalized-sibling admission, sound-base descent) | `e349dc4e` (DAG-ancestry *finality agreement*) is critical for multi-parent finalization liveness; the floor-derivation correctness feeds REBUILD §C-base |
| `0bb91b22` | **read bonds/rewards endpoints from FS(LFB)** not keep-one post-state | consumer-side correctness |
| `7ed761ab` | **fresh-joiner latest-message fix** + graceful not-bonded skip | consensus liveness under concurrent bonds; faithful Scala-bug port |
| `4fdbd6aa` | **poison-tolerant shared-LMDB test lock** | stops a flake cascade |
| `ad0081d7` | **LFS state-sync hardening** | networking resilience (join_all, deadline budget, byzantine reason) |
| `e7efb39d` | **active-committee weighting** (`block.bonds = active(FS) ∩ bonds(FS)`) | committee read from signed block; closes the active-vs-bonded finality fracture |
| `97767045` | **genesis-sourced FT threshold** (`getFaultToleranceThreshold` PoS getter) | required for node-identical floor |
| `4f63cb82` (part) | **live committee** (`active ∩ bonds ∩ live`; `recent_producers`, GRACE/LIVENESS windows; drop dead-stake from FT denominator) | the COMMITTEE half of the eager commit — sound + orthogonal to the base regression. Isolate from the base half |
| `4f63cb82` (part) | **LFS horizon requester** (275 lines) | LFS feature; verify standalone |
| `54`/`8ef27c30` (part) | **merge/seal sub-stage timers + diagnostic gating** | the `DAG_MERGE_*_TIME_METRIC` timers are useful; keep, drop the seal-specific probes |
| `66`/`e2ce06d1` | **CI: enable user-contract-concurrency integration test** | keep the test wired |
| `3bfbdca5` (part) | **map_cell recovery/convergence repro** (the green-gate) | PRESERVE. The rest of `3bfbdca5` is pool-model test alignment → see §E |

## B. REBUILD — the core concepts (implement fresh per the design; branch = reference)
Do NOT cherry-pick these; the listed commits are where to *read* the prior attempt.

- **BASE = finalized-floor committed state, one fold.** Reference: `3499b39e`/`48`/`82dfb222`/`33`/`ec2cb29f` (floor derivation), `b90498e9` (advance-only monotone principle). Target: `base_state = floor.post_state`, `scope = closure(parents)\closure(floor)`. NOT the tip (`4f63cb82` base half — DROP), NOT a separate seal fold. **[landed on v2]**
- **RECOVERY = pool shape + record-driven oracle.** Reference: `a6b61a65` (pool shape), `1ba7943a` (don't-evict-pending-on-accept principle), the FloorData ledger idea (`31`/`c4013314` — reference only). Target: pool retained until the merge's canonical record says done (`body.deploys`/`body.rejected_deploys` in `closure(floor)`); gas-cell `recovered_deploy_effect_in_base` kept only for foldable number cells. **[landed on v2]**
- **MERGE = multiplicity-correct DAG-ancestry conflict detection, no bolt-on.** Reference: `b6bd4ec6` (DAG-ancestry conflict = concurrency — PRESERVE this principle). Target: fix `combine` multiplicity so `resolve_conflicts` rejects N−1 of N natively; DELETE the single-value-cell serialize pass (`b1ec0ed6`/`44267c8a`). Main_parent's unfinalized writes are participants (consequence of the floor base). **[landed on v2]**
- **STATUS** (`433594f5`/`631b756d`): deleting the buggy sig-scan resolver = PRESERVE; effect-presence status for number cells = PRESERVE; for single-value cells = REBUILD onto the record. Expose effect-level deploy finalization on REST/gRPC lookup responses. **[landed on v2]**

## C. DROP — regressed / superseded / scaffolding
- Eager base (`4f63cb82` base half), "FS=committed-at-tip" (`b8e7b181`).
- Gas-cell oracle as single-value-cell recovery truth (keep only for number cells).
- The seal reformulations + scaffolding: `previous_finalized_cut` (`88d8af4f`), PLAY (`6d5319d1`), structural diff-fold seal `merge3_*` (`f533bf86`, `b90498e9` fold parts, `a6a35dcf`, `b6bd4ec6` seal-pass), the separate `floor_seal.rs` fold + `FloorData` ledger + floor-state KV store, the sealed-fate ledger/`FloorFateResolver` (`f0886852`/`4cff5d02`).
- `FinalSet`/`FinalContext` enforcement window (`3499b39e` part, `0ed32a36` deletes it → the **deletion** is PRESERVE).
- Single-owner re-proposal gate removal (`b9c38177`): **do NOT preserve the removal** — RE-ADD the gate (`37`/`03df496d`). Deploys gossip and the record is cross-node-visible, so without the gate every node re-proposes the same loser → spam.
- Diagnostic-only log-level commits (`ef2559ee`, `91d58070`, `2efc7711`), test_utils API catch-up (`57557966`).

## D. IN-STAGING (don't re-port)
- Logging standardization (commits 1-24, merge-base `0ca238fa`).
- FFI/dep cleanup (`d2bdddd4`), rustfmt (`51f07e6b`, `437072ab`), CI/docs already in staging.

## E. The 2 new reference commits
- `3bfbdca5` — test alignment to the **pool model** → MOSTLY REWORK/DROP; the obsolete-test deletions stay deleted; the repro carved out → PRESERVE (§A).
- `1634e842` — instrumentation + unverified closeBlock experiment → port the trace probes selectively; the experiment is UNVERIFIED → re-evaluate or drop.

## Open / verify-before-porting
No applicable reference keeper is currently left open for this v2 branch. If new reference commits
are fetched later, repeat the split review for MIXED rows before cherry-picking any hunk.
