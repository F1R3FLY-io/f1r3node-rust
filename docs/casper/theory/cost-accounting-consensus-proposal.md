# Cost Accounting and Casper: Remaining Gaps and an Order-Then-Execute Proposal for Cost-Accounted (v6) Shards

**Note (DR-131).** Host-work metering is removed (`75de2dc39`). The host-work content of this document is historical. Restore: `git revert 75de2dc39`.

| Field | Value |
|---|---|
| Status | Proposal for discussion with the Casper team. Not ratified. |
| Date | 2026-10-09. Amended on 2026-10-12 after an audit of every claim against the code. Amended on 2026-10-13 for the reduction scheduler, the candidate pick, and record and replay (see the amendment note below). |
| Audience | The Casper team of F1R3FLY, the user (the developer who directs this work), and Greg (L. G. Meredith), the author of the cost-accounting papers |
| Code basis | Branch `feature/cost-accounted-rho` at `e38ff0de8`. It contains `dev`, the integration branch of the F1R3node repository, at `93e38575e`. Every `file:line` reference is at `e38ff0de8` unless the text names another commit. The first version of this document used the commit `17e07307f`, and the profiles of §9 come from that commit. |
| Paper basis | The F1R3FLY publications repository at revision `0bf7817` |
| Authority | The [ratification status ledger](../design/cost-accounting-ratification-status.md) applies. An implementation record does not ratify a Casper change. |

**Amendment (2026-10-13, the scheduler, the candidate pick, and record and replay).** The user decided two points on 2026-10-13, and this amendment applies them. It follows the user's standing rule: "stick with dev's version of everything that is not directly related to the correctness or completeness of cost-accounting". Replaced text stays in `<details>` blocks titled "Text before the amendment of 2026-10-13". The [genesis resource policy](cost-accounting-impl/genesis-resource-policy.md) keeps its earlier text in the same way.

- **The scheduler stays.** The single-order reduction scheduler of `deterministic_reduction.rs` stays. The new §3.7 analyzes it. It covers what the scheduler does, why it was added and what the papers say. It also covers the effect on E1 to E4 and the dependency of dev's file I/O work. This document does not propose a return to dev's racing evaluation. On 2026-10-13 the user removed the analysis of what a return would entail (the former §3.7.6).
- **The candidate pick.** The user ordered on 2026-10-12 that RSpace pick a candidate at random, as dev does, with two fixes at the replay boundary. The new §3.8 explains why the pick does not affect the correctness of cost accounting.
- **Record and replay in §7.** Section 7 no longer assumes a deterministic executor. One executor runs the ordered envelopes and publishes its record. The other nodes replay that record and attest. A wrong attestation is shown by replaying the record. This works whether the scheduler stays or goes, and with a fixed or a random candidate pick.
- **Changed sites.** The amendment changes §1.4 (conclusion 2 and the new conclusion 6), §1.5 and the reading guide. It also changes §3.6, §6.3, §6.7, §7.1 to §7.3, §7.6 to §7.13, §8.1, §9.2, §10 and §12.
- **Basis.** New code citations use the code basis `e38ff0de8` unless they name another commit. The file `deterministic_reduction.rs` is the same at `e38ff0de8` and at `717a33b16`.
- **Diagrams.** Two new diagrams show the scheduler against racing evaluation (§3.7.1) and order-then-execute with record and replay (§7.1). The diagrams of §8.2 and §10 predate this amendment. Their executor box covers both execution and replay.

## Contents

1. [Purpose and summary](#1-purpose-and-summary)
2. [Background for Casper readers](#2-background-for-casper-readers)
3. [Today's pipeline](#3-todays-pipeline)
   - [3.7 The single-order reduction scheduler](#37-the-single-order-reduction-scheduler), added on 2026-10-13
   - [3.8 The candidate pick inside RSpace](#38-the-candidate-pick-inside-rspace), added on 2026-10-13
4. [What is being implemented now](#4-what-is-being-implemented-now)
5. [Prior implementation on feature/casper-cost-accounting-completion](#5-prior-implementation-on-featurecasper-cost-accounting-completion)
6. [Remaining gaps](#6-remaining-gaps)
7. [The recommended architecture: order-then-execute for v6 shards](#7-the-recommended-architecture-order-then-execute-for-v6-shards), with record and replay since 2026-10-13
8. [Change analysis](#8-change-analysis)
9. [Performance analysis](#9-performance-analysis)
10. [Phased plan](#10-phased-plan)
11. [Open decisions](#11-open-decisions)
12. [References](#12-references)

## Reading guide

**Evidence tags.** Each claim carries one tag or more.

| Tag | Meaning |
|---|---|
| [C] | Code, with a `file:line` reference. The code is at HEAD unless the text names another commit. HEAD is the commit `e38ff0de8` of the code basis. |
| [D] | The text of a repository document, such as a decision record, at a named commit |
| [P] | Paper text, with line numbers |
| [M] | Measured data, with its source and its limits |
| [S] | A logged prompt or reply of codex, the coding agent of the earlier branch (§5). It carries the session identifier (ID) and the line number. |
| [I] | An inference of this document. The reader must judge it. |

**Names used in this document.** This document gives many items a short identifier (ID). The table below defines each family of IDs in one place. The text also defines each ID at its first use, with a short plain name.

| Name | Meaning | Defined in |
|---|---|---|
| Acronyms | API: application programming interface. AUC: area under the curve. CBC: correct by construction. COMM: communication event. CPU: central processing unit. DAG: directed acyclic graph. DOI: digital object identifier. LFB: last finalized block. LFS: last-finalized-state synchronization. LMD-GHOST: latest-message-driven greedy heaviest-observed subtree. PoS: proof of stake. PR: pull request. TLA+: temporal logic of actions. | §2.1, and at first use |
| v6, legacy | A v6 shard runs protocol version 6, with cost accounting and offered envelopes. A legacy shard runs dev's Casper path. | §2.1 |
| P1, P2, ST | The three governing papers of Greg. The next table gives their titles. "P1" never names a phase. | Papers table below |
| P3, CS | Two further papers of Greg that §3.7 cites on scheduling and nondeterminism. They are not governing papers. The papers table gives their titles. Added on 2026-10-13. | Papers table below |
| HEAD, commit IDs | HEAD is the commit `e38ff0de8`. A hexadecimal string such as `1d325b996` is the ID of a git commit. The word "session" marks the ID of a codex session instead (§5). | This guide |
| E1 to E4 | Greg's four expectations: E1 merge logic, E2 user alternatives, E3 keep accepting blocks, E4 performance | §1.2 |
| RB-1, RB-3a | The two fixes at the replay boundary that come with the random candidate pick. The restoration plan calls them E1 and E3-a. This document renames them, because E1 to E4 name Greg's expectations here. Added on 2026-10-13. | §3.8 |
| (a), (b), (c), (c)+(b), (d1), (d5) | The candidate designs. "(a)+(b)" in §11 combines (a) and (b). | §1.4 |
| Epic 8946, bug 11004 | Two items in pgmcp, the work tracker of this effort: the epic "Complete offered-funded production cost-accounting integration", and the global cost-cursor lock bug. The cost-cursor lock fix closed the bug on 2026-10-10. | §2.1, §2.4 |
| G1, G2, G3, G6, G9 | Gap labels of epic 8946. G1: the funding check before execution. G2: several offers per block. G3: installer funding. G6: the recovery of offered envelopes. G9: the v6 merge rule. | §2.1 |
| G-5 | The rule of G1 that an on-chain purse funds nothing before execution. It is a gap fill of the implementation map of G1. | §4.1 |
| DR-*n* | Decision record *n*. DR-119 and DR-120 are the records of the cost-cursor lock fix and of the interim v6 merge rule. | §12.3 |
| Class I, class S, class D | The three conflict classes under cost accounting: independent, same purse, and shared data | §2.4 |
| F1 to F8 | The eight fast-path conditions of the v6 merge rule | §4.4 |
| C1 to C10 | The ten changes that an independent arbiter required of an earlier version of the v6 merge-rule design | §4.4 |
| HIGH-*n*, MEDIUM-*n*, LOW-*n* | The findings of the third independent check of that design, with their levels | §4.4 |
| CA-P-*n* | Row *n* of the cost-accounting conformance catalog | §5 |
| GAP-E1 to GAP-E4, GAP-i to GAP-iv | The remaining gap against one expectation, or against one item of the new model of P1. The items are (i) explicit transaction boundaries, (ii) concurrent acceptance, (iii) throughput, and (iv) merge via signatures and cost accounting. | §6.0 |
| A-LIN | The assumption that each LFB lies on the main chain of every later LFB | §7.5 |
| Q-1 to Q-6 | The technical questions for the Casper team | §7.13 |
| Q-7 | The technical question on records. Added on 2026-10-13. | §7.13 |
| The scheduler | The single-order reduction scheduler of `deterministic_reduction.rs`. It orders the RSpace operations inside one deploy evaluation. Added on 2026-10-13. | §3.7 |
| Record, canonical record, $`\mathrm{executed}(b)`$ | The record of one run of a finalized block $`b`$, the record that the canonical order fixes for $`b`$, and the root after that record. Added on 2026-10-13. | §7.7 |
| T1 to T4, M1 to M5 | The profiled tests, and the measurements that are still needed | §9.1, §9.3 |
| Phase 0, Phase 1, Phase 1b, Phase 2, Phase 3 | The phases of the plan. S, M, L and XL are their relative sizes. | §10 |
| D-0 to D-9 | The open decisions. Q1 to Q3, without a hyphen, are the three sub-questions of D-0. | §11 |
| EPIC-021, issue #*n*, PR #*n* | An epic of the task board `docs/ToDos.md`, and an issue or a pull request of the repository `F1R3FLY-io/f1r3node-rust` on GitHub | §8.1, §8.3 |
| Soak run IDs | GitHub Actions run numbers of soak tests, for example 37224478325 | §9.1 |
| CLAIM-CASPER-SOAK-001 to 008 | The eight claims of the Casper soak harness in `docs/claims/` | §8.1 |
| D-F2, T-10 | The work item of DR-118, which reuses the reducer's measurement of each introduction, and the fork-choice exclusion theorem of the slashing design | §6.10, §8.3 |
| Symbols | Each mathematical symbol is defined where it first occurs. The main ones are the supply $`\Sigma_s`$ and the demand $`\Delta_s`$ of a signature $`s`$, and the original root $`R_0`$ (§2.1). Others are the forced tokens $`\kappa`$ (§2.2) and the time ratio $`\rho`$ (§9.3). | At first use |

**Papers.** The three governing papers are in the F1R3FLY publications repository. Line numbers refer to revision `0bf7817`.

| Short name | Title | Path in the publications repository |
|---|---|---|
| P1 | *Cost-Accounted Rho Calculus: A Spectral Decomposition of Phlogiston* (L. G. Meredith, May 2026) | `cost-accounting/cost-accounted-rho.tex` |
| P2 | *Continued Interactive GSLTs and the Cost Endofunctor: A construction one level up from cost-accounted Rholang* (L. G. Meredith, May 2026) | `cost-accounting-as-monad/continued-gslt-cost-v2.tex` |
| ST | *Spacetime from Cost: A functor from cost-accounted ciGSLTs to measured causal sets* (L. G. Meredith, June 2026) | `cost-spacetime/spacetime-functor.tex` |
| P3 | *Knotted Topoi: the lift of the knotted set-theoretic universe, and fully abstract denotational semantics for the category of graph-structured lambda theories* (L. G. Meredith, June 2026) | `knotted-topoi/knotted-topoi.tex` |
| CS | *From the Axiom of Choice to Fair Scheduling: Toward a Curry–Howard Correspondence for Nondeterminism, in which choice principles are the types and fair schedulers are the terms* (L. G. Meredith, research note, June 2026) | `choice-types/choice-scheduling.tex` |

P3 and CS are not governing papers. §3.7 cites them on scheduling, and their line numbers also refer to revision `0bf7817`. The amendment of 2026-10-13 added them.

"P1:344-366" means lines 344 to 366 of P1.

**Code.** A reference such as `runtime.rs:346` uses the short file name of this key. Paths are relative to the repository root.

| Short name | Path |
|---|---|
| `block_creator.rs` | `casper/src/rust/blocks/proposer/block_creator.rs` |
| `interpreter_util.rs` | `casper/src/rust/util/rholang/interpreter_util.rs` |
| `runtime.rs` | `casper/src/rust/rholang/runtime.rs` |
| `runtime_manager.rs` | `casper/src/rust/util/rholang/runtime_manager.rs` |
| `validation_dispatcher.rs` | `casper/src/rust/engine/multi_parent_casper/validation_dispatcher.rs` |
| `finalization_runner.rs` | `casper/src/rust/engine/multi_parent_casper/finalization_runner.rs` |
| `dag_merger.rs` | `casper/src/rust/merging/dag_merger.rs` |
| `block_index.rs` | `casper/src/rust/merging/block_index.rs` |
| `conflict_set_merger.rs` | `casper/src/rust/merging/conflict_set_merger.rs` |
| `deploy_chain_index.rs` | `casper/src/rust/merging/deploy_chain_index.rs` |
| `clique_oracle.rs` | `casper/src/rust/safety/clique_oracle.rs` |
| `estimator.rs` | `casper/src/rust/estimator.rs` |
| `floor.rs` | `casper/src/rust/finality/floor.rs` |
| `validate.rs` | `casper/src/rust/validate.rs` |
| `epoch.rs` | `casper/src/rust/epoch.rs` |
| `metrics_constants.rs` | `casper/src/rust/metrics_constants.rs` |
| `execution.rs` | `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution.rs` |
| `family_selection.rs` | `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/family_selection.rs` |
| `offered_block.rs` | `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/offered_block.rs` |
| `production_limits.rs` | `casper/src/rust/util/rholang/costacc/production_limits.rs` |
| `acceptance.rs` | `casper/src/rust/util/rholang/acceptance.rs`. §5 cites the 8,258-line file at the commit `f9bd3895d`. HEAD has a different 541-line file at the same path, which the commit `068d51c65` created. |
| `gate.rs` | `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/gate.rs` |
| `replay.rs` | `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/replay.rs` |
| `settlement.rs` | `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/settlement.rs` |
| `direct_wallet_funding.rs` | `casper/src/rust/util/rholang/costacc/direct_wallet_funding.rs` |
| `vault_payer.rs` | `casper/src/rust/util/rholang/costacc/vault_payer.rs` |
| `v6_merge/mod.rs` | `casper/src/rust/util/rholang/costacc/v6_merge/mod.rs` |
| `fast.rs` | `casper/src/rust/util/rholang/costacc/v6_merge/fast.rs` |
| `ordered.rs` | `casper/src/rust/util/rholang/costacc/v6_merge/ordered.rs` |
| `SystemVault.rho` | `casper/src/main/resources/SystemVault.rho` |
| `MakeMint.rho` | `casper/src/main/resources/MakeMint.rho` |
| `PoS.rhox` | `casper/src/main/resources/PoS.rhox` |
| `offered_funded_api_test.rs` | `casper/tests/api/offered_funded_api_test.rs` |
| `offered_gateway_slot_api_test.rs` | `casper/tests/api/offered_gateway_slot_api_test.rs` |
| `offered_prepaid_api_test.rs` | `casper/tests/api/offered_prepaid_api_test.rs` |
| `offered_admission_api_test.rs` | `casper/tests/api/offered_admission_api_test.rs` |
| `offered.rs` | `models/src/rust/signed_phlo_deploy/offered.rs` |
| `block_metadata.rs` | `models/src/rust/block_metadata.rs` |
| `cost_protocol_limits.rs` | `models/src/rust/cost_protocol_limits.rs` |
| `CasperMessage.proto` | `models/src/main/protobuf/CasperMessage.proto` |
| `block_dag_key_value_storage.rs` | `block-storage/src/rust/dag/block_dag_key_value_storage.rs` |
| `deterministic_reduction.rs` | `rholang/src/rust/interpreter/deterministic_reduction.rs` |
| `reduce.rs` | `rholang/src/rust/interpreter/reduce.rs`. A citation of dev or of another commit names that commit. |
| `trace/replay.rs` | `rholang/src/rust/interpreter/accounting/native_runtime/checked_operations/trace/replay.rs`. The short name `replay.rs` names the casper file above. |
| `trace/replay/session/operations.rs` | `rholang/src/rust/interpreter/accounting/native_runtime/checked_operations/trace/replay/session/operations.rs` |
| `budget_trace.rs` | `rholang/src/rust/interpreter/accounting/native_phlo_rules/execution/budget_trace.rs` |
| `replay_authority.rs` | `rholang/src/rust/interpreter/accounting/native_runtime/replay_authority.rs` |
| `p_input_normalizer.rs` | `rholang/src/rust/interpreter/compiler/normalizer/processes/p_input_normalizer.rs` |
| `operation_context.rs` | `rspace++/src/rspace/operation_context.rs` |
| `ops_produce.rs` | `rspace++/src/rspace/rspace/ops_produce.rs` |
| `candidate_order.rs` | `rspace++/src/rspace/candidate_order.rs` |
| `metered.rs` | `rspace++/src/rspace/replay_rspace/native_candidate/metered.rs` |
| `native_session/operations.rs` | `rspace++/src/rspace/replay_rspace/native_session/operations.rs` |
| `rholang_merging_logic.rs` | `rholang/src/rust/interpreter/merging/rholang_merging_logic.rs` |
| `dispatch.rs` | `rholang/src/rust/interpreter/dispatch.rs` |
| `accounting/mod.rs` | `rholang/src/rust/interpreter/accounting/mod.rs` |
| `authority.rs` | `rholang/src/rust/interpreter/accounting/authority.rs` |
| `delta_sigma.rs` | `rholang/src/rust/interpreter/accounting/delta_sigma.rs` |
| `lexical.rs` | `rholang/src/rust/interpreter/accounting/lexical.rs` |
| `phlo_bounds.rs` | `rholang/src/rust/interpreter/accounting/phlo_bounds.rs` |
| `phlo_controls.rs` | `rholang/src/rust/interpreter/accounting/phlo_controls.rs` |
| `funding_minimax.rs` | `rholang/src/rust/interpreter/accounting/monetary_allocation/funding_minimax.rs` |
| `desugar.rs` | `rholang/src/rust/interpreter/compiler/normalizer/cost_accounting/desugar.rs` |
| `match.rs` | `rspace++/src/rspace/match.rs` |
| `space_matcher.rs` | `rspace++/src/rspace/space_matcher.rs` |
| `rspace_interface.rs` | `rspace++/src/rspace/rspace_interface.rs` |
| `DeterministicParallelReduction.v` | `formal/rocq/cost_accounted_rho/theories/DeterministicParallelReduction.v` |
| `EndToEndAuthority.v` | `formal/rocq/cost_accounted_rho/theories/EndToEndAuthority.v` |
| `HeldCapacityGuard.v` | `formal/rocq/cost_accounted_rho/theories/HeldCapacityGuard.v` |

"DR-*n*" names record *n* of the [decision records](cost-accounting-decision-records.md). Each decision record gets a short description at its first use, and §12.3 lists them all. Seven records are newer than `17e07307f`. DR-119 records the cost-cursor lock fix (§4.5). DR-120 records the interim merge rule for cost-accounted (v6) shards (§4.4). Four records cover the steps of the funding check before execution (§4.1). DR-125 records the fix of bug 11008, a call of an unknown method that the node did not charge.

Since the amendment of 2026-10-13, §3.7 and §3.8 also cite the older records DR-31, DR-74, DR-75, DR-78, DR-79, DR-84 to DR-86, DR-96, DR-100 and DR-102.

**Status markers.** Each item of §4 carries a dated status line. The orchestrator, the agent session that coordinates this work, updates these lines. §5 reports the finished review of the earlier branch.

---

## 1. Purpose and summary

### 1.1 Why this document exists

The user gave this direction:

> "Implement what you can, cleanly, without a lot of changes to casper, then thoroughly document the remaining gaps and how you recommend they be resolved. I will discuss your proposal with the casper team. Include analyses of what will need to be changed."

This document has three aims:

1. It records the work that the implementation lane (the cost-accounting implementation work on this branch) does now with a small Casper footprint (§4).
2. It lists the gaps that remain against Greg's expectations and against the papers (§6).
3. It recommends a resolution and analyzes the Casper changes that the resolution needs (§7, §8).

### 1.2 Greg's four expectations

Greg is the author of the cost-accounting papers. The user relayed his four expectations verbatim on 2026-09-28 (codex session `01a0e3fe`, lines 10134 and 10260). [S]

| ID | Short name | Expectation (verbatim) |
|---|---|---|
| E1 | Merge logic | "The new cost-accounting changes eliminate the majority if not all the merge logic." |
| E2 | User alternatives | "users may decide which alternatives they want which eliminates merging." |
| E3 | Keep accepting blocks | "You no longer need to run to completion -- you can just keep running and keep accepting blocks." |
| E4 | Performance | "the majority of the performance issues in F1r3node are caused by the existing merge logic and that the cost-accounting changes should eliminate most of them." |

E1 came as a question that ended in "correct?". The user gave E2 as the reason for E1. The user also asked codex to "verify against the model" and to decide whether it "can be and should be the case". [S]

### 1.3 What the papers say

P1 starts from two defects of the current node. [P]

- **Run-to-completion.** RSpace, the tuple-space store of the node, "accepts a deployment, executes it to termination (or until phlogiston is exhausted), commits the resulting state changes, and only then accepts the next deployment" (P1:259-262).
- **Merge analysis.** "Merge analysis is expensive, complex, and a persistent source of bugs." (P1:300-301). "Wasted computation is the norm, not the exception." (P1:306-307).

P1 replaces run-to-completion with "concurrent acceptance gated by linear resource proofs" (P1:313-314). Conflicts "are detected *at acceptance time* through the linear resource proof" (P1:346-348). [P]

**Scope of the papers.** P1 states its arrival rules for deployments that "arrive at a validator" (P1:2247). P1 frames the merge burden as combining states that "different validators" or speculative parallel runs computed (P1:290-298). It claims to replace that analysis (P1:344-354). It does not say how a multi-validator block DAG (the directed acyclic graph of blocks) orders acceptances. Every mapping of P1 onto Casper in this document is therefore an interpretation. [P][I]

P1 lists an implementation path of four phases, and no phase changes consensus (P1:1716-1730). P1 also says that block validity becomes "a syntactic predicate" (P1:1711-1712). [P]

### 1.4 Bottom line

This document compares six candidate designs. §6.9 and §7.12 define them in full. A v6 shard is a shard of protocol version 6, which runs cost accounting (§2.1). G1 is the funding check before execution, and G2 allows several offered deploys in one block (§4).

| Name | Design |
|---|---|
| (a) | Execute-then-merge with G1, G2 and the interim v6 merge rule (§4) |
| (b) | Execute-then-merge with a parallel batch executor, G1, G2 and the cost-cursor lock fix (§4.5) |
| (c) | Order-then-execute on v6 shards with a sequential executor |
| (c)+(b) | Order-then-execute on v6 shards with the parallel batch executor |
| (d1) | Purse lanes: one ordering lane for each purse |
| (d5) | Single-parent v6 shards |

The conclusions, with their basis:

1. **Execute-then-merge, the pipeline at HEAD, meets E1 only in part and cannot meet E3.** [I] A proposer must merge the parent states before it executes. A validator must replay a block before it accepts it (§3).
2. **Order-then-execute on cost-accounted (v6) shards meets all four expectations.** [I] E4 still needs confirmation by measurement. E2 is met if the canonical order may pick the winner among alternatives. Sub-question Q3 of decision D-0 (what Greg means, §11) asks Greg about this. Consensus orders the accepted envelopes, which are signed user deploys. One executor runs the finalized order, applies the acceptance gate of P1 (§2.2) before each envelope, and publishes its record. The other nodes replay that record and attest its root (§7, amended on 2026-10-13).

   <details>
   <summary>Text before the amendment of 2026-10-13</summary>

   A deterministic executor runs the finalized order and applies the acceptance gate of P1 (§2.2) before each envelope (§7).

   </details>

3. **Order-then-execute remains Casper.** [I] It keeps the justifications of Casper CBC (correct by construction), the fork choice, the clique-oracle finality and the equivocation slashing. It changes when state is computed and where the validator weights come from (§7.8, §7.9).
4. **E4 is not yet proven.** [M][I] The data of the August soak test (a long load test of a validator network) agree with E4. After dev's later fixes, no per-node stage separates failing soak sessions from passing ones. Dev's analysis points to the finality round length and to blocks that wait for missing parents (§9).
5. **The earlier branch does not change this assessment.** [C][I] A review of the earlier branch `feature/casper-cost-accounting-completion` found that its tip met none of E1 to E4 (§5). It met E2 in part, but only inside one proposer's block and only under a narrow reading in which funding decides. No gap of §6 closes, and §7 to §11 do not change.
6. **Record and replay does not depend on the reduction scheduler.** [C][I] This conclusion was added on 2026-10-13. The single-order scheduler stays, by the user's decision of that day. §3.7 analyzes it. RSpace returns to dev's random candidate pick, with two fixes at the replay boundary (§3.8). Execution is therefore not deterministic. Agreement comes from the record, as it does today between a producer and its validators.

Two further gaps affect every design (§6.10). The first is the committed host-work caps, which are limits on deterministic node work per deploy. Today they reject the testnet log initializer of Embers, the F1R3FLY blockchain application programming interface (API). The second is unmetered continuations on the settlement log channels.

| Design | E1 merge logic | E2 user alternatives | E3 keep accepting blocks | E4 performance |
|---|---|---|---|---|
| (a) | Partial | Partial | Unmet | Marginal |
| (b) | Unmet | Partial | Partial | Partial |
| (c) | Met on v6 | Met if Q3 picks the canonical order | Met | Largest gain, not yet measured |
| (c)+(b) | Met on v6 | Met if Q3 picks the canonical order | Met | Largest gain, not yet measured |
| (d1) | Partial | Met for funding | Unmet | Small |
| (d5) | Mostly | Unmet | Unmet | Mixed |

### 1.5 What this document asks of the Casper team

1. Review the gap analysis (§6) and the change analysis (§8).
2. Decide the open decisions D-0 to D-9 together with the user and Greg (§11).
3. Answer the technical questions Q-1 to Q-6 (§7.13), and Q-7, which the amendment of 2026-10-13 adds.
4. Choose the owner of the order-then-execute work (decision D-9, ownership).

---

## 2. Background for Casper readers

### 2.1 Terms

This section defines the cost-accounting, Casper and merge terms. The summary in §1 uses some of them before this section.

| Term | Definition | Source |
|---|---|---|
| Deployment, deploy | A signed term that a user submits for execution. It is the unit of financial atomicity. | P1:2234-2240 |
| Phlogiston, phlo | The cost unit of Rholang. It gates every deploy behind a token balance of a signature. | P1:145-147 |
| Signature | In P1 a signature is a kind of channel name. A signed term $`\{P\}_s`$ charges each of its communications to the signature $`s`$. | P1:203-206, 218-222 |
| Token stack | A first-class term $`S ::= () \mid s : S`$ that holds tokens of signature $`s`$. | P1:223-227 |
| Token supply $`\Sigma_s`$ | The depth of the token stacks of signature $`s`$ in the ambient composition. | P1:2029-2045 |
| SystemVault | The genesis contract that holds the purses | `SystemVault.rho` |
| Purse | A token stack located at a channel name. Only processes that hold the name can draw on it. P1 calls it a located resource stack. In the code, one purse is one physical custody key of SystemVault. | P1:575-583, DR-65 (same-block offers that share a purse) |
| Funding slot | An unforgeable channel that a contract creates as a funding surface. "Anyone who deposits tokens" on the slot pays for its continuation. | P1:1557-1587 |
| Compound signature | A joint authority $`s_1 \circ s_2`$. Every component signer must sign, and the process fires only with tokens of every component. The compound holds one combined token stack, a joint purse. | P1:380-397, 887-888. `Sig::And` (`accounting/mod.rs:3150`), `compound_cost_signatures` (`authority.rs:387`) |
| Lollipop | A transfer signature, written `[s1 -o s2]` in Rholang. In `{for(y <- x){P}}[s1 -o s2]`, the signature `s1` funds the communication, and `s2` funds the continuation `P`. | P1:1273-1293. `desugar.rs:7-13, 61-80` |
| Cost share | Settlement splits one charge among the eligible sources of one envelope. It shares equally where it can and uses lexicographic minimax otherwise. The user approved this rule on 2026-09-10. | `funding_minimax.rs:63`, [lexicographic minimax funding](cost-accounting-impl/lexicographic-minimax-funding.md) |
| Owner price ceiling | The highest price that one required owner signed. The applicable price must not exceed the ceiling of any required owner. | `phlo_controls.rs:110, 125, 251`, `phlo_bounds.rs:48-50`, [signed price consent](cost-accounting-impl/signed-price-consent.md) |
| Demand $`\Delta_s`$ | The number of $`s`$-signed layers that a deployment can force, over its whole call graph. | P1:2002-2027 |
| Linear resource proof | The inequality $`\Sigma_s \geq \Delta_s`$ for every signature $`s`$ of a deployment. Each token is used exactly once. Here *linear* refers to linear logic. | P1:2047-2058, 2318-2326 |
| Acceptance, the gate | The validator computes the demand before any execution, compares it with the supply, and then accepts or rejects. | P1:2188-2213 |
| Hold | The part of a purse that acceptance commits to one deployment until settlement. Other deployments cannot use it. | P1:2301-2302, DR-64 (the funding check before execution) |
| Settlement | The step after execution that charges the measured use, transfers the fee and refunds the unused hold. | P1:2132-2136, DR-38 (reserve and settle as one native step), DR-64 |
| Cost cursor | Revision and position cells in SystemVault for one charge scope. Each charged settlement advances them. An existing cursor takes no lock. A first use takes one of 256 bucket creation locks (DR-119). | `SystemVault.rho:63-72, 146-160, 186-229` |
| Offered envelope | The v6 user-deploy format: a body, a funding intent, a signed phlo limit and a signed phlo price. | `offered.rs:11-21` |
| v6 shard | A shard whose genesis adopts the protocol-6 offered-funded resource policy. Its user deploys are offered envelopes only. | DR-66 (the activation of protocol 6), `validation_dispatcher.rs:124-130` |
| Legacy shard | A shard with a legacy genesis. It runs dev's Casper path. | DR-66 |
| Original root $`R_0`$ | The authenticated state root that a candidate funds from. | DR-64 |
| LFB | The last finalized block of a node. | `finalization_runner.rs:351-370` |
| Main parent | The first parent, `parents[0]`. Fork choice selects it. | DR-15 (run-to-completion and the merge dispatcher), item 4 |
| Past cone $`\mathrm{past}(B)`$ | The block $`B`$ and all of its ancestors. | Standard |
| Epoch | A fixed run of block numbers. The shard configuration carries the epoch length. | `epoch.rs:27` |

Consensus, runtime and tooling terms:

| Term | Definition | Source |
|---|---|---|
| Casper CBC | The correct-by-construction Casper consensus of F1R3node. Stake sets a validator's weight. | P1:3767-3770 |
| Block DAG | The directed acyclic graph of blocks. A block can name several parents. | `CasperMessage.proto:329-334` |
| Justifications | The latest message of each validator, as the sender of a block saw them | `CasperMessage.proto:295` |
| LMD-GHOST | The latest-message-driven, heaviest-subtree fork-choice rule. It adapts the GHOST (greedy heaviest-observed subtree) rule of [Sompolinsky and Zohar 2015]. | `estimator.rs:3-23` |
| Clique oracle | The safety oracle that finalizes a block when a clique of agreeing validators holds enough stake | `clique_oracle.rs` |
| RSpace | The tuple-space storage engine of F1R3node, keyed by channel names | P1:1694-1696 |
| COMM | One RSpace communication event: a send that matches a receive | DR-32 (the COMM is the only execution-cost event) |
| PoS contract | The proof-of-stake contract that holds the bonds | `CasperMessage.proto:601-603` |
| IntegerAdd channel | A channel that holds one number. A merge combines concurrent changes to it by addition. | `rholang_merging_logic.rs:102-136` (the addition), `:237-267` (one value) |
| LFS | Last-finalized-state synchronization: a joining node fetches the blocks and the tuple space of the LFB. | `casper/src/rust/engine/lfs_tuple_space_requester.rs` |
| Reset, checkpoint | Before a node plays or replays a block, it resets RSpace to the pre-state root. After execution it creates a checkpoint, which commits the state and gives the post-state root. | `rspace++/src/rspace/history/history_repository_impl.rs`, `docs/ToDos.md:241-246` |
| Host-work budget, host-work caps | A per-deploy budget of deterministic node work, such as admission, authority analysis and witness checks. Its caps are consensus constants. It charges no tokens. | [host-work budget](host-work-budget.md) |
| Rocq, TLA+ | Rocq is the proof assistant formerly named Coq. TLA+ (temporal logic of actions) is a specification language. TLC is its model checker. | `formal/` |
| Negative control | A deliberately broken model or test that must fail. It shows that a check is not vacuous. | `docs/Glossary.md` |
| Soak test, soak run | A long load test of a multi-validator network in continuous integration. Its sessions pass or fail against gates, such as a limit on the finalization latency. | `docs/ToDos.md:74-106` |
| A/A calibration | Two runs of the same build that measure the noise of an experiment | Phase 0 (evidence) of §10 |
| AUC | Area under the curve: the probability that a failing soak session has the higher value of a metric | `docs/ToDos.md:205` |

Terms of dev's merger, which §3 and §4.4 use:

| Term | Definition | Source |
|---|---|---|
| Deploy chain, branch | A deploy chain is a maximal chain of mutually dependent deploys. It is the smallest unit that a merge keeps or rejects. A branch is a set of mutually dependent chains. | [Merge-algebra glossary](merge-algebra/merge-algebra-glossary.md) |
| Merge base, merge scope | The merge base is the block whose post-state the merge starts from. The merge folds the changes of the blocks in its scope onto that state. | [Finalized-floor glossary](finalized-floor/finalized-floor-glossary.md) |
| Floor | For a block $`B`$, the highest ancestor of the parents of $`B`$ that the clique oracle certifies as finalized over the justifications of $`B`$. The finalizer derives a floor from its live view in the same way. | [Finalized-floor glossary](finalized-floor/finalized-floor-glossary.md) |
| Event-log index | The index of the produce and consume events of each chain. The merger finds conflicts with it. | [Merge-algebra glossary](merge-algebra/merge-algebra-glossary.md) |
| Mergeable channel | A channel whose concurrent changes fold, for example by addition on an IntegerAdd channel | [Merge-algebra glossary](merge-algebra/merge-algebra-glossary.md) |
| Folded mixing | One branch changes a mergeable channel by a folded diff, and another branch writes the same channel as a plain value. Dev's merger rejects the mix. | `dag_merger.rs:438-462` |
| Over-fill | A merge result in which a single-value cell would hold more than one datum | [Merge-algebra glossary](merge-algebra/merge-algebra-glossary.md) |
| Rejected records | The deploys that a merge rejects. A block records them, and every validator recomputes them. | `CasperMessage.proto:554-575` |
| Option search | The adjudication of dev's merger, which legacy shards keep. It lists the sets of branches that it could reject. It picks one by prior losses and then by cost. On v6 shards one ordered pass replaces it (DR-120). | `conflict_set_merger.rs:870-980`, `dag_merger.rs:1870-1882` |
| Pinned chain, settled chain | A chain whose effects are already in the state that the merge builds on. A merge never rejects it. | `conflict_set_merger.rs:137-147`, `dag_merger.rs:1434` |
| Late chain | A chain whose merge window has closed. The merge rejects it and every chain that depends on it. | `dag_merger.rs:988-992` |
| Prior losses | The number of earlier merges that rejected a chain. A chain with more losses gets priority. | `conflict_set_merger.rs:1018-1024` |
| Lineage closure | After a merge rejects chains, it also rejects each unpinned chain whose block descends from a block with a rejected chain. | `dag_merger.rs:1913-1974` |
| Merge index | The index of a validated block that later merges read. The node builds it eagerly, right after validation, when the shard allows more than one parent. | `validation_dispatcher.rs:340-375` |
| Settled-effect probe | A walk along a state lineage that decides if the effect of a deploy signature is already committed. The merge calls it to protect settled content. It is also called a settled-signature probe. | [Settled-effect probe claim](../../claims/settled-effect-probe-equivalence.md) |

Epic 8946 is the epic "Complete offered-funded production cost-accounting integration" in pgmcp, the work tracker of this effort. Its work items have short gap labels, and this document uses them.

| Name | Work | Record |
|---|---|---|
| G1 | The funding check before execution | DR-64 |
| G2 | Several offered deploys in one block | DR-65 |
| G3 | Installer funding of a stored continuation | DR-68 (an installer-signed continuation draws on the installer's purse) |
| G6 | Recovery of rejected offered envelopes | DR-116 (recovery carries offered envelopes) |
| v6 merge rule | The interim merge rule for v6 shards. Its gap label is G9. | DR-120 |
| Cost-cursor lock fix | The fix of the global cost-cursor lock bug, pgmcp bug 11004 (§2.4) | DR-119 |

### 2.2 The acceptance protocol of P1

P1 defines acceptance as five steps (P1:2188-2213). [P]

1. The validator receives a signed deployment $`\{D\}_c`$ and looks up the token stack of the client $`c`$.
2. "**Before** executing any part of the deployment", the validator computes the demand by static analysis (P1:2191-2193).
3. The validator computes the supply $`\Sigma_c`$.
4. If the supply covers the demand, the deployment is accepted. "Every substitution that begins will complete." (P1:2204-2205).
5. Otherwise the deployment is rejected "without executing any part of the deployment. No state change occurs. No tokens are consumed." (P1:2210-2212).

Data-dependent control flow uses a conservative bound over the branches $`\beta`$ of a deployment $`D`$ (P1:2138-2152):

```math
\Delta_c^{\max}(D) \;=\; \max_{\beta \in \mathrm{Branches}(D)} \Delta_c(\beta),
\qquad
\text{accept}(D) \iff \Sigma_c \geq \Delta_c^{\max}(D).
```

After execution the refund is $`\Delta_c^{\max}(D) - \kappa`$, where $`\kappa`$ is the number of tokens that the run forced (P1:2149-2151). A construct that the analysis cannot resolve contributes an "unknown" demand. The validator then rejects "unless the supply exceeds the known lower bound plus a configurable safety margin" (P1:2077-2080). DR-64 uses the signed phlo limit as that margin.

### 2.3 Arrival in sequence and simultaneous arrival

P1 gives one worked example (P1:2245-2287). [P] Alice has three tokens. Two copies of one payment each need three tokens.

| Arrival | Decision | Lines |
|---|---|---|
| In sequence | The first copy is accepted because $`3 \geq 3`$. The second copy sees $`\Sigma = 0`$ and is rejected before any step. | P1:2252-2262 |
| Together | The validator treats both as one parallel composition. $`\Delta = 3 + 3 = 6 > 3 = \Sigma`$, so neither payment executes. | P1:2271-2282 |

P1 states the general rule as a theorem of linear logic: "Two deployments competing for the same tokens are two proof obligations competing for the same linear hypotheses. At most one can succeed." (P1:2311-2313). [P]

### 2.4 How cost accounting changes conflicts

Today the merger discovers conflicts after execution, from the event logs of the executed deploys. P1 replaces that analysis with three classes. The signatures decide two of them. Shared data channels mark the third (P1:356-366). [P]

> "deployments signed by *different* signatures draw from *disjoint* token pools and cannot conflict. They may be executed in parallel and their results composed without merge analysis. Deployments signed by the *same* signature (or overlapping compound signatures) compete for the same token pool and are serialized by the linear proof --- again without merge analysis. The only case requiring attention is deployments that interact via shared data channels, and even here the cost-accounting structure provides a natural serialization order." (P1:356-366)

![Activity diagram of the three conflict classes under cost accounting. Two deployments D1 and D2 enter. The first question asks if they draw on a common purse. A common purse means the same signature or overlapping compound signatures. If yes, they are class S, and the linear proof serializes them. In sequence, the first is accepted and the second is rejected if the remaining supply is short (P1:2245-2269). Together, they form one parallel composition, and both are rejected if the supply is short (P1:2271-2287). An indigo note says that P1 states both rules for one validator only. If they share no purse but share a data channel, they are class D, which an order serializes (P1:363-366). Otherwise they are class I. They run in parallel and compose without merge analysis (P1:356-359). A red note describes HEAD e38ff0de8 on a v6 shard. The fast path composes a conflict-free scope, and one ordered pass decides the rest. Both run after execution. Along one chain, the check before execution decides class S. Another red note says that since DR-119, sibling settlements conflict only on a shared cursor scope or a shared first-use bucket. That note names the fixed global cost-cursor lock bug, pgmcp bug 11004. A legend explains the colours. It also defines P1, the deployments D1 and D2, their demands, and the supply of the common purse. It defines the classes S, D and I, HEAD e38ff0de8, v6, DR-119, pgmcp bug 11004 and DAG.](diagrams/consensus-proposal-conflict-classes.svg)

*Source: [`diagrams/consensus-proposal-conflict-classes.puml`](diagrams/consensus-proposal-conflict-classes.puml). Render it with `./render.sh consensus-proposal-conflict-classes.puml` in `docs/casper/theory/diagrams/`.*

The three classes have a precise basis in ST. A redex is a reducible term, that is, a step that can fire. Two co-enabled redexes $`r_1`$ and $`r_2`$ are independent under the Bernstein conditions [Bernstein 1966] on their write sets $`\mathrm{foot}(r)`$ and read sets $`\mathrm{rfoot}(r)`$ (ST:367-383):

```math
\mathrm{foot}(r_1) \cap \mathrm{foot}(r_2) = \emptyset,
\qquad
\mathrm{foot}(r_1) \cap \mathrm{rfoot}(r_2) = \emptyset,
\qquad
\mathrm{rfoot}(r_1) \cap \mathrm{foot}(r_2) = \emptyset.
```

Independent redexes commute by a diamond lemma (ST:413-425). Two redexes that both pop one located-stack head are "*competing*", which is a race (ST:393-399). A maximal conflict-free set of events "is one consistent history", and each branch of a race "is the choice of which spacetime" (ST:1252-1257). [P]

For Casper readers, the consequence is direct. [I]

| Class | What decides it under cost accounting | What decides it today on a v6 shard |
|---|---|---|
| I, independent | Nothing. The results compose. | The v6 fast path, after execution, when its eight conditions hold (§4.4). It reads the event-log index and runs no rejection logic (DR-120). |
| S, same purse | The gate, in arrival order | Along one chain, the check of G1 before execution at $`R_0`$ (§4.1). Across sibling blocks, the v6 ordered pass, after execution (DR-120). |
| D, shared data | An order. Under §7 it is the canonical order. | The v6 ordered pass in its strict order of branches, after execution (DR-120) |

The global cost-cursor lock bug (pgmcp bug 11004) added a fourth effect until 2026-10-10. A global lock put every pair of charged offered siblings into a false class S conflict. DR-119 removed it (commit `7ab849b56`). An existing cursor now takes no lock (`SystemVault.rho:186-194`). A first use takes one of 256 bucket locks (`SystemVault.rho:63-72, 196-218`). Two settlements now conflict only when they share a cursor scope, or when first uses on both sides fall in one bucket. The second case is a residual false conflict. Its probability is near $`4/256`$ for two new cohorts (DR-119). [C] DR-115, the record that a failed offered deploy keeps its settlement in the merge index, describes the old lock under "Merge outcomes". An amendment of 2026-10-12 to DR-115 records the change. [D]

### 2.5 What the papers do not specify

The following points are outside the papers. This document marks every rule for them as an inference. [P][I]

- One acceptance order across several validators. P1 states arrival rules for one validator (P1:2247, 2272-2273).
- The lag between consensus and execution. P1 speaks of RSpace that accepts deployments "at any time" (P1:327-328), not of blocks.
- "Keep accepting blocks" (E3) extends P1 from RSpace to blocks. The extension is Greg's.
- An envelope-level group of alternatives. No paper defines one.
- A text search of six companion papers found no rule for block ordering or block-state merging. They are the papers on choice types, consensus types, rent, history and cost, virtual tokens and radical fault tolerance.

---

## 3. Today's pipeline

This section describes the code at HEAD on a v6 shard. Legacy shards run the same path with legacy deploys. Their merges keep dev's rule (`MergeRule::Dev`, `v6_merge/mod.rs:51-57, 81-103`).

![Sequence diagram of today's execute-then-merge pipeline on a v6 shard at HEAD e38ff0de8. The participants are the signing user, the proposer, the merge, the play runtime, the validator, the replay and the finalizer. In the proposal phase, the signing user sends a signed offered envelope. The proposer selects at most one offer. It merges the parent states first. On a v6 shard the merge composes a conflict-free scope, or else one ordered pass chooses the survivors, after execution. A red note says that the proposer cannot run a deploy before this merge. The play runtime then checks the funding at the original root R0, executes the single envelope and settles it. It replays the envelope and runs the system deploys. A red note says that a v6 block holds one envelope and that the legacy loop plays each deploy to termination. In the validation phase, every validator recomputes the merge. It compares the pre-state, the rejected records and the state facts. It replays the block to termination. A legacy block waits for the one-permit ReplayLock, and an offered replay takes no permit. A red note says that acceptance waits for execution. The validator builds the eager merge index and adds the block to the DAG. In the finality phase, the finalizer derives the floor of the live view. It reads the weight map from the main parent's bonds, which execution wrote into the block. A legend explains the colours. It also defines HEAD e38ff0de8, v6 shard, offered envelope, R0, CloseBlock, ReplayLock, DAG, LFB, floor, and the file and line notation.](diagrams/consensus-proposal-today-pipeline.svg)

*Source: [`diagrams/consensus-proposal-today-pipeline.puml`](diagrams/consensus-proposal-today-pipeline.puml). Render it with `./render.sh consensus-proposal-today-pipeline.puml` in `docs/casper/theory/diagrams/`.*

### 3.1 Proposal

1. The proposer selects at most one offered envelope. Up to `ordinary_cap − 1` further envelopes become alternates. The proposer tries them in canonical order when the selected one is rejected. The rest wait for later proposals (`block_creator.rs:1847-1916`, the rule at `:1910-1916`, the retry at `:4100-4113`). [C]
2. `compute_deploys_checkpoint_envelopes` logs "merge parents, then run deploys" (`interpreter_util.rs:914`). It merges first (`interpreter_util.rs:932-942`) and plays afterwards (`interpreter_util.rs:997-1007`). [C]

### 3.2 Merge

1. `compute_parents_post_state` (`interpreter_util.rs:1214`) dispatches on the number of parents (`interpreter_util.rs:1246-1321`). A single parent gives its post-state only when its lineage holds the derived floor. Otherwise the block re-bases through the full merge (`interpreter_util.rs:1313-1321`). [C]
2. With several parents, or after a re-base, the block takes the full merge path. On a v6 shard it first tries the fast path, which composes a conflict-free scope without the rejection logic (`interpreter_util.rs:1995-2016`, `fast.rs:144`). Otherwise it calls `dag_merger::merge_with_rule` (`interpreter_util.rs:2018-2036`, `dag_merger.rs:863`). The rule is `OfferedV6` when a parent or a scope block holds an offered deploy (`v6_merge/mod.rs:81-103`). [C]
3. On a legacy shard, the merger chooses the rejected branches by an option search after execution (`conflict_set_merger.rs:870-980`). It groups every pair of branches that share any mergeable channel (`:906-918`). With pinned branches it searches exhaustively (`:879-889`). Production pins the settled chains (`dag_merger.rs:1434`). On a v6 shard, one ordered pass in a strict total order $`K`$ on branches replaces the search (`dag_merger.rs:1870-1882`, `ordered.rs:173`, §4.4). It also runs after execution. [C]
4. The merge results are consensus content: the pre-state root, the rejected records, the applied-from-scope set and the merge base (`CasperMessage.proto:554-575`). [C]

### 3.3 Play and run-to-completion

1. `compute_state_envelopes` (`runtime.rs:327-392`) accepts exactly one envelope (`runtime.rs:346`). A second envelope is an error (`runtime.rs:364-373`). [C]
2. The offered path first checks the funding before execution (`offered_block.rs:220-236`, `execution.rs:495-521`). The check estimates the client's demand with the static analysis (`gate.rs:169`, `:423-442`). It rejects the offer with no effect when the holds do not fit the supply at $`R_0`$. DR-123 (the check before execution of an offered deploy) and DR-124 (the live check of the client's demand) record this check. The path then executes the candidate and settles the measured use (`offered_block.rs:259-280`). A run that drains the funds of its own settlement is a charged user failure (DR-65 item 5, commit `88f4176db`). [C]
3. The producer replays its own candidate before it publishes (`offered_block.rs:315-323`). It then runs the system deploys (`offered_block.rs:361-370`). [C]
4. The legacy loop plays each deploy to termination before the next one (`runtime.rs:455-476`). A play budget can defer the remaining deploys to a later proposal (`runtime.rs:455-457`). [C]

### 3.4 Validation and replay

1. `run_validation_steps` (`validation_dispatcher.rs:84-334`) checks the block summary (`:99-118`) and the offered-only rule (`:124-130`). It then runs the checkpoint step. [C]
2. The checkpoint step recomputes the merge from the block's own justifications (`interpreter_util.rs:420-435`). It compares the pre-state (`:463-472`), the rejected records (`:473-520`) and the state facts (`:521-534`). Then it replays the block (`:540-547`). An offered replay first runs the same check before execution at $`R_0`$ (`offered_block.rs:585-606`, `replay.rs:1642`). [C]
3. A node replays one legacy block at a time. The `ReplayLock` is a semaphore with one permit (`runtime_manager.rs:198-205`). Of the block replays, only the legacy replay takes it (`:1348`, acquired at `:1227`). The wait has its own metric (`:1226-1229`). The replay of a block with an offered deploy returns earlier and takes no permit (`:1317-1333`). [C]
4. Offered replay requires "one isolated user candidate" (`offered_block.rs:784-788`). [C]
5. After validation, every block gets a merge index when the shard allows more than one parent (`validation_dispatcher.rs:340-375`). The guard is at `:345`. [C]

### 3.5 Finality

1. Every `finalization_rate` blocks, the node starts the finalizer (`finalization_runner.rs:662-701`). [C]
2. The finalizer derives the floor of the live view (`finalization_runner.rs:493-560`, `floor.rs:517-643`). It adopts a new LFB only when the derived state contains the settled effects of the current LFB (`floor.rs:615-638`). [C]
3. The clique oracle reads the weight map of the target's main parent (`clique_oracle.rs:202-220`). That map comes from the block's `bonds` (`block_metadata.rs:141-162`), which execution writes into the post-state fields (`CasperMessage.proto:597-605`). [C]

### 3.6 Where run-to-completion and the merge happen

| Place | What waits, or what is repeated | Code |
|---|---|---|
| Block play | Each deploy runs to termination before the next | `runtime.rs:455-476`, `:346` |
| Proposal | Play waits for the merge of the parents | `interpreter_util.rs:932-942` |
| Validation | Acceptance waits for the merge recomputation and the replay | `interpreter_util.rs:402-557` |
| Replay | One legacy replay at a time on each node. An offered replay takes no permit. | `runtime_manager.rs:198-205, 1227, 1317-1333` |
| Producer | The producer replays its own candidate | `offered_block.rs:315-323` |
| Native frontier | One conflict component for each native frontier inside one deploy. §3.7 analyzes this point (added on 2026-10-13). | `deterministic_reduction.rs:1068-1072, 1120-1122` |
| After validation | A merge index for every block | `validation_dispatcher.rs:340-375` |

**Note on DR-15.** DR-15 item 1 states that run-to-completion "was never ported" and that the acceptance gate of DR-11 is live through `admit_by_funding`. DR-11 is the record of a static linear-proof gate at block assembly. At HEAD, the code above plays and replays each deploy to termination. No file under `casper/src` or `node/src` calls `admit_by_funding` or `static_authority_plan`, so the gate of DR-11 is not live. [C] Since the commits `02d01541b` and `e64261134`, a different check runs before execution in play and in every replay. It is the check of DR-123 and DR-124, and it calls `delta_sigma::estimate` (`gate.rs:26-28, 423-442`). It checks one envelope against $`R_0`$ and admits nothing concurrently. [C] The merge `1d325b996` restored dev's Casper code on 2026-10-01 (§5). DR-120 amended the outcome of DR-15 on 2026-10-10 (`b496df9d8`). An amendment of 2026-10-12 to DR-15 corrects item 1. [D]

### 3.7 The single-order reduction scheduler

**Status on 2026-10-13.** This section is new. It is analysis only. The user decided on 2026-10-13 to keep the scheduler.

The user gave this direction:

> "keep it and amend the proposal document to address Greg's concerns with information about the scheduler, why it was added, and what reverting to the randomized scheduler would entail (but include just the analysis, do not propose reverting to dev's version but make that option possible)"

Greg's concerns are his four expectations (§1.2) and the papers on which they rest. This section therefore states what the scheduler does and why it exists. It measures the scheduler against the papers and against E1 to E4. It does not propose a return to dev's racing evaluation. On 2026-10-13 the user removed the analysis of what a return would need (the former §3.7.6). [I]

The source is a design study of the removal of the scheduler, `target/scratch/scheduler-removal-design-v1.md`. That study is a local artifact and is not in the repository. This document checked its paper quotes at revision `0bf7817` and its main code citations at `e38ff0de8`. Where the two differ, this document follows the code. [I]

#### 3.7.1 What the scheduler does

`deterministic_reduction.rs` replaces dev's racing evaluation inside one deploy evaluation. Its design document is [deterministic parallel reduction](cost-accounting-impl/deterministic-parallel-reduction.md), which this section calls the scheduler document. The table defines its terms. [C][D]

| Term | Meaning | Source |
|---|---|---|
| Participant | One live causal branch of a reducer evaluation. A split of a parallel composition replaces the parent participant with its child participants. | Scheduler document, lines 31-34. `deterministic_reduction.rs:92-133` |
| Intent | A typed request for one RSpace `produce` or `consume`. It carries the pattern, the continuation, the data and the cost authority. | Scheduler document, lines 36-38 |
| Complete frontier | The point at which every live participant waits on exactly one intent or has completed. No intent commits before that point. | Scheduler document, lines 40-42. `deterministic_reduction.rs:827-841` |
| Causal operation order | The pair of a session identity and a tree path. The operation of participant `P` at step `s` has the path `P·(s, 0)`. | Scheduler document, lines 44-48. `deterministic_reduction.rs:82-90` |
| Footprint | The channels, the joined channels and the authority regions that an intent can touch | Scheduler document, lines 58-63 |
| Conflict component | A connected component of the graph whose edges join the intents with intersecting footprints | Scheduler document, lines 70-73 |

The scheduler runs the step below for each deploy evaluation. The pseudocode follows the scheduler document (lines 116-128) and the code. [C][D]

```text
⟨one scheduler step⟩ ≡
  on submit(participant, intent):
      bind the intent to the next causal operation path of the participant
      mark the participant as waiting
      if every live participant waits or has completed:        -- a complete frontier
          take every submitted intent
          footprint(i) ← channels(i) ∪ joined channels(i) ∪ authority regions(i)
          if native execution is active:
              add the common key [2, 0] to every footprint      -- one component
          components ← the connected components of intersecting footprints
          run the intents of each component in causal order,
              and run the components at the same time
          send the completions in causal order, and resume their participants
```

Three properties of the code matter for cost accounting. [C]

1. **One order for a native deploy.** Under native execution, `insert_authority` adds the footprint key `[2, 0]` to every intent (`deterministic_reduction.rs:1120-1122`). Every native frontier is therefore one conflict component, and a debug assertion checks it (`:1068-1072`). The intents of a native deploy run one at a time, in causal order. This document calls that order the single order.
2. **Legacy deploys bypass the queue.** `DeterministicRSpace` passes the produce and consume calls of a legacy budget straight to RSpace (`deterministic_reduction.rs:1318-1323, 1338-1340`).
3. **The evaluation boundary.** An evaluation holds a shared permit. Checkpoints, resets and rollbacks take the exclusive permit (`deterministic_reduction.rs:49-62, 1207-1210`). A native evaluation waits for its detached tasks, and an incomplete cut is a platform failure (`:433-438`).

**How dev differs.** [C][D]

- Dev spawns each branch of a parallel composition as a Tokio task and awaits every handle (dev `a06e3e7b3`, `reduce.rs:318-337`). The dev commit `0f3943d64` of 2026-10-13 still does so (`reduce.rs:426-441`).
- The tasks race. Per-channel locks serialize the operations on one channel, and arrival order decides the rest.
- Play picks a candidate at random (dev `ops_produce.rs:251-261`).
- Replay does not race against play. `ReplayRSpace` "uses the play event log to force the exact same COMMs during replay, regardless of evaluation order" (dev `docs/rholang-evaluation-order.md:25-26`).
- That dev document opens with the heading "Rholang Is Non-Deterministic By Design" (line 5).

![Activity diagram of one deploy evaluation under two designs. A deploy body with parallel branches enters and splits into two flows. The left flow, in lilac, is the single-order scheduler of this branch. It splits the participant into child participants with causal paths. Each child submits one RSpace intent and waits. A complete frontier forms when every live participant waits or has completed. Native execution adds the key [2, 0] to every footprint, so each native frontier is one conflict component. The intents then run one at a time in causal order, the single order of a native deploy. A green note lists four things that the scheduler supplies to cost accounting today. They are equal replay host work at live map sizes (DR-78, DR-79 step 5, DR-100) and operation names by causal path (DR-74, DR-84 to DR-86). They are also order-dependent decisions without a record in the G3 design draft, and checkpoints that never see a partial evaluation. The right flow, in amber, is dev's racing evaluation. It spawns each branch as a Tokio task and awaits every handle. The tasks race, per-channel locks serialize one channel, and arrival order decides the rest. Both flows join. RSpace picks one matching candidate, by the canonical order at e38ff0de8 and at random after the restoration of dev's pick. Play writes the record, the event log and the native cost evidence. Replay follows the record. A grey note quotes dev's document: Rholang is non-deterministic by design, and replay forces the same COMMs regardless of evaluation order. A legend explains the colours and defines participant, intent, complete frontier, footprint, the key [2, 0], conflict component, native deploy, COMM, DR-n, G3, HEAD and dev.](diagrams/consensus-proposal-scheduler-vs-racing.svg)

*Source: [`diagrams/consensus-proposal-scheduler-vs-racing.puml`](diagrams/consensus-proposal-scheduler-vs-racing.puml). Render it with `./render.sh consensus-proposal-scheduler-vs-racing.puml` in `docs/casper/theory/diagrams/`.*

#### 3.7.2 Why it was added, and what it supplies today

**Origin.** [C][D]

- The scheduler entered with the merge `325bf0d1a` of 2026-08-29. Neither parent of that merge had the file.
- Both parents ran the branches of a parallel composition as racing Tokio tasks: `286b1bb1e` (`reduce.rs:268-273`) and dev `58d43cb6d` (`reduce.rs:306-307`).
- Item 4 of the merge message names "deterministic parallel reduction without globally serializing validators".

**The stated reason.** [D]

- The node "has to choose one replayable outcome when several parallel branches compete for the same RSpace datum, continuation, join, or purse region" (scheduler document, lines 14-16).
- The counterexample of the scheduler document is `@"x"!(1) | @"x"!(2) | for (@y <- @"x") { @"out"!(y) }`. Arrival order can give the consumer either value (lines 79-96).
- The scheduler document says that the scheduler "closes the observed validator disagreement in which two honest nodes placed the same deployment in different blocks because parallel RSpace arrival selected different residual state" (lines 219-221).

**A correction.** The scheduler document says that "The publications require reduction-order-independent cost" (line 13). That overstates the papers (§3.7.3). The papers require that the cost of a run be a function of the redexes that the run forces. They do not require one canonical run. [P][I]

**What it supplies to cost accounting today.** Four guarantees rest on the scheduler. [C][D]

| Guarantee | How the scheduler supplies it | Records |
|---|---|---|
| Every validator's replay of one block charges the same host work | Some charges read live map sizes: the produce counter, the replay authority maps and the observation reserve (`replay_authority.rs:151, 207-212, 224-230`). The single order makes these sizes a function of the record. The guarantee holds on one host and in one process. Bugs 10814 and 10812 remain (amendment of 2026-10-12 to DR-100). | DR-78, DR-79 step 5, and DR-100: "the accept/reject verdict at a cap boundary decides consensus" |
| Play and replay give each operation the same name | An operation is named by its participant path and a step counter (`deterministic_reduction.rs:82-90`). A task-local value carries the name into RSpace (`operation_context.rs:206-214`). The evidence keys attempts and journal rows by that path. | DR-74, DR-84, DR-85, DR-86 |
| Decisions that depend on order need no record | The G3 design draft v8 lets replay recompute "skips, filters and denials from its live state" in "the scheduler's operation order" (`target/scratch/p1/g3-implementation-map-v8.md:594, 661`, a local artifact). G3 has no code yet (§4.3). | The G3 design, not yet a record |
| Checkpoints never see a partial evaluation | The evaluation permit, the drain of detached tasks and the complete-cut check (`deterministic_reduction.rs:49-62, 433-438`). The scheduler document gives the boundary design (lines 178-208). | Scheduler document |

**A latent hang.** Native replay waits for the recorded predecessors of an operation without a bound (`trace/replay.rs:422-436`, `native_session/operations.rs:150-165`). A forged journal can place a row before its causal origin. That row can then wait forever, and the driver blocks inside the wait. The design study reasoned this from the code, and no test has run it. [C][I]

**The candidate pick.** The scheduler orders operations, not candidates (§3.8). After the restoration of the random pick, the scheduler no longer makes outcomes deterministic. In the counterexample above, the consumer then gets 1 or 2 at random. Play and replay still agree, because replay follows the record (§3.8). [C][I]

#### 3.7.3 What the papers say

No paper requires a deterministic schedule. The papers let each run decide which interactions fire, and they bound the cost of every run. The evidence follows. [P][I]

- **No rule picks among enabled redexes.** P1's rules "are closed under parallel context and structural equivalence" (P1:928-929). "In every rule, a communication fires only when matching tokens are present" (P1:832-833).
- **P1 uses the word "scheduler" once, as an analogy.** The bootstrap step "is free infrastructure, analogous to the operating system's process scheduler" (P1:3371-3373).
- **Each run has one step order, and its stack records it.** The temporal stack "is a free monoid (a list), *never* commutative, because reduction is sequential even when interaction is parallel" (P2:573-575). Phlogiston is consumed "in the order recorded by" the stack (P2:862-865). No rule requires that every run have the same order.
- **Competition remains.** "two receivers $`I^1, I^2`$ may both be near the same $`J`$, so competition for a stack remains possible ... Exclusivity, where required, is an additional matter for the type discipline" (P2:1360-1367). P2 declines to fix one rule: "Rather than hardwire a single choice, we let the discipline be *checked*" (P2:1406-1407). For an $`n`$-ary join, "the choice is downstream of the binary endofunctor" (P2:1650-1660).
- **A process denotes a set of transitions.** In P3, "The behaviour functor is $`\mathsf{B}(R)=\mathcal{P}(\partial T_K\times R)`$ ... the process universe is $`\mathsf{Proc}=\nu\mathsf{B}`$, and bisimilar processes are equal in it" (P3:254-255). No schedule appears in the denotation.
- **A scheduler resolves what a system leaves open.** "A scheduler resolves the nondeterminism a system leaves open: at each point of contention it selects which enabled interaction fires" (CS:160-161). Linear logic "types the structure of interaction and is silent on its resolution" (CS:231-234).
- **Cost is per run, with a bound and a refund.** For one set of forced redexes, the cost does not depend on order. Demand is additive, $`\Delta_s(T \mid U) = \Delta_s(T) + \Delta_s(U)`$ (P1:2019-2020). The consumed cells equal the forced redexes (P2:580-588). Across runs the cost can differ: "which interactions occur --- and so how many tokens of which signatures are drawn --- depends on the run" (P1:2090-2093, also P2:1490-1494). The papers answer with a bound and a refund: "The refund is computed from the *actual* run, which is observable" (P1:2154-2160).
- **The records agree.** The validator model of DR-31 has "different arrival orders, reducer schedules", and its schedule set includes "a different event set with a different cost". All accepted validators agree through the certified witness (DR-31). [D]

| Question | Answer | Evidence |
|---|---|---|
| Does a paper require a deterministic schedule? | No | P1:928-929, P2:1360-1367, P3:254-255 |
| Does a paper require a random choice? | No | The same lines |
| Does a paper allow a random choice? | Yes, as one resolution among many. P1 itself races a token lookup against a timeout. | P1:3216-3220, P2:1741-1743 |
| Who decides among authorized competitors? | The declared discipline of the program: its capabilities, surfaces, types and funding. No paper names a runtime rule. | P2:1360-1367, P2:1406-1408, CS:326-328 |

P1 states its parallelism for deployments (P1:356-359, §2.4), not for the steps inside one deployment. No paper describes replay. Section 2.5 already records that one acceptance order across validators lies outside the papers. [P]

#### 3.7.4 Effect on Greg's expectations

| Expectation | What the scheduler does | Effect |
|---|---|---|
| E1, merge logic | Nothing. The v6 merge rule reads event-log sets and IntegerAdd sums (DR-120, §4.4). | Neutral |
| E2, user alternatives | It orders operations inside a deploy, not deploys. It never lets a user choose. | Neutral |
| E3, keep accepting blocks | It holds every native operation of a deploy until every live branch waits (`deterministic_reduction.rs:827-841`). It makes each native frontier one conflict component (`:1068-1072, 1120-1122`). A slow pure branch delays its frontier (scheduler document, lines 203-208). | It hinders E3 inside a deploy. |
| E4, performance | It adds a frontier barrier, intent queues, driver tasks and the bookkeeping of detached tasks. It serializes native RSpace work. | It probably hinders E4 inside a deploy. Nobody has measured it. |

The effect stays inside one deploy, because consensus never calls the scheduler. §3.6 lists the native frontier as a serialization point, and GAP-E3 cites it (§6.3). The profiles of §9.1 show that the branch does 15 to 23 % more work than dev on three legacy tests, outside the merge. They do not show how much of that work the scheduler does. [M][I]

The scheduler once supported one premise of §7: a deterministic executor. That premise also needed a canonical candidate pick, which the restoration removes (§3.8). Since 2026-10-13, §7 no longer needs the premise (§7.1). [I]

#### 3.7.5 Dev's file I/O work depends on the scheduler

- The branch `origin/fileio` contains the merge `325bf0d1a`, so it has the scheduler. [C]
- Mike Stay's commit `0f7bfb089` of 2026-09-11 added `park_external_during` to the scheduler. A file-lock wait keeps its participant running, so the frontier never forms: "Deploy deadlocks until eval-test-source timeout" (the message of `0f7bfb089`). The helper removes the participant from the frontier while the wait runs. [C][D]
- The file I/O handlers of `origin/fileio` call the helper, for example at `rholang/src/rust/interpreter/io/handlers_helpers.rs:174` and `:521-524` (at `3e1a807ad`, the tip of `origin/fileio`). [C]
- Dev prepares a port of that copy. Dev commit `70c2a3dc1` (Mike Stay, 2026-10-07) says: "Slice 5.101 completes the 4-piece scaffolding needed to port `deterministic_reduction.rs` (1437 LOC) in a future slice". [D]
- Comments in dev plan the call sites (`io/handler_trait/spawn_blocking.rs:19-42` and `io/handlers/lock/mod.rs:26-42` under `rholang/src/rust/interpreter/`, at dev `0f3943d64`). Dev ignores one test until the port lands (`casper/tests/genesis/contracts/fileio_examples_spec.rs:19-23, 500`). [C]

Four points follow. [I]

1. A removal on this branch would run against the direction of dev's file I/O work. The design study asked the user to decide this point first. The user decided on 2026-10-13 to keep the scheduler.
2. Dev's port is the copy of 1,437 lines on fileio. This branch's file has 1,448 lines before its test module. If the branch later takes dev's version, it must check that the copy keeps the four guarantees of §3.7.2.
3. This branch carries dev's file-lock handlers without `park_external_during` (`rholang/src/rust/interpreter/io/handlers/lock/fs_lock_range.rs:33`). If a native deploy can reach them, a parked waiter could stall a native frontier, as on fileio before `0f7bfb089`. This document did not check that reachability.
4. Dev runs no scheduler, so the hang of the ignored dev test must have another cause on dev. This is an inference that no test has checked.

### 3.8 The candidate pick inside RSpace

**Status on 2026-10-13.** This section is new. The user ordered on 2026-10-12 that play pick an RSpace candidate at random, as dev does. The implementation lane is restoring the pick now. At the code basis, the canonical order still runs: `ops_produce.rs:292-297` calls `canonical_candidates`. [C]

**What the pick is.** An operation can find several matching candidates on a channel, and it must pick one. Dev shuffles the candidates with an unseeded random generator (dev `ops_produce.rs:251-261`). This branch had replaced the shuffle with an order by source hash. DR-75 gives the reason: "Cost accounting needs play and native replay to choose the same candidate". [C][D]

**Why the canonical order is not needed.** The restoration plan (`target/scratch/m34-restoration-plan.md`, a local artifact) gives the analysis below. This document checked its main code citations. [C][I]

- Every production replay follows the recorded pick. Native replay takes the recorded COMM as its candidate identity (`trace/replay/session/operations.rs:125-130`). It skips every continuation that does not match that identity (`metered.rs:968-971`). Legacy replay is dev's code.
- No charge depends on candidate order beyond the pick itself. That covers phlo charges, byte charges, receipts, evidence fields and settlement inputs (restoration plan, §2).
- Play does not count the candidate scan (`candidate_order.rs:137-167`). Replay host work must agree only between replays (DR-79 item 4, DR-100, DR-102). Native replay keeps the canonical order as its own internal order (`metered.rs:224-229`).
- Both roles certify by replay (`offered_block.rs:316, 808`). An unfixed boundary case therefore quarantines an honest deploy under DR-72. It does not publish an invalid block.

**Two fixes at the replay boundary.** Replay must work out again what the record leaves out. That matters in two cases. The restoration plan calls their fixes E1 and E3-a, and this document calls them RB-1 and RB-3a. [C][I]

| ID | Case | Fix |
|---|---|---|
| RB-1 | A join that names one channel twice at run time. The normalizer blocks only the repeats in the source (`p_input_normalizer.rs:483`), and substitution can still repeat a channel. The COMM records the set of produces, not which datum met which pattern, so replay must rebuild the assignment. Dev has this defect too. | `extract_data_candidates_rollback` (`space_matcher.rs:100`) orders the list of a repeated channel by source hash, with the incoming datum first. Compared with dev, play's pick changes only for such joins. |
| RB-3a | The native check of a Stored outcome. Replay runs a greedy match again and treats any match as a mismatch (`native_session/operations.rs:227-230, 416-422`). Under a random play order, a commit guard makes that result depend on the order. | The check skips every continuation whose commit guard can veto. Replay then cannot reject a false Stored claim on a guarded continuation. Dev's replay never rejects a Stored claim (restoration plan, §4). |

Two further cases need two candidates with identical random states, so they cannot occur in practice (restoration plan, §4). [I]

**Effect on cost accounting.** The pick does not affect correctness. Replay follows the record, as it does on dev. RB-1 and RB-3a close the only two places where replay rebuilds a choice that a random play can change. [I]

**Relation to E2.** [I]

- In this document's reading, E2 concerns deployments. Funding defines the alternatives, arrival order picks the winner, and a choice by a condition stays inside one deployment (§6.2).
- The pick inside RSpace is not a user decision under either rule. Under the canonical order, a user wins a race by grinding hashes: "about n+1 offline hashes to come first among n candidates" (restoration plan, Q5).
- A random rule gives no honest party an edge. A dishonest producer can bias both rules.
- If E2 also covers which waiting continuation a datum meets, then both rules conflict with E2. Sub-question Q3 of D-0 asks Greg the deployment-level form of that question (§11).

**Relation to §7.** Under the random pick, two honest executors can reach different roots. Since 2026-10-13, §7 therefore records and replays (§7.1). [I]

---

## 4. What is being implemented now

**Status on 2026-10-12.** The user approved the designs of all five items on 2026-10-10. G1 is committed through the live check (step G1-6) and the held-capacity guard for one offer per block (step G1-7). The check of G1 is live in play and in every replay (§4.1). The v6 merge rule and the cost-cursor lock fix are committed. Of G2, only the code motion G2-1 is committed. G3 is not started. The orchestrator updates this section.

The user directed the lane to "Implement what you can, cleanly, without a lot of changes to casper". The five items below follow that rule. Each one changes Casper only at a v6 seam or in cost-accounting code.

Two ownership boundaries apply to this work.

1. **Casper.** The lane changes Casper only where cost accounting requires it. General Casper findings go to the Casper team as reports. The lane does not fix them.
2. **Stack safety.** Another team owns stack safety for the interpreter: the MeTTaIL and F1R3Lang language effort, on the branches `feature/mettail`, `feature/f1r3lang-mettail-only` and `integration/f1r3lang-cost-accounted-rho-20261005`. The lane backports that team's existing code where cost accounting needs it, with a citation. It writes new stack-safe code only for cost-accounting modules that no branch covers.

### 4.1 G1: the funding check before execution (DR-64)

**Status: approved on 2026-10-10 after two independent checks. The fixture step, the steps G1-1 to G1-4b, G1-6 and G1-7 are committed. The check runs before execution in play and in every replay.** The design document is [the linear pre-execution cost estimate](cost-accounting-impl/linear-cost-estimate.md).

- **Problem.** Before step G1-6, the offered path executed the candidate and checked the funding afterwards (`offered_block.rs:222-280` at the commit `72de6b52a`). [C] Since G1-6, the check runs before any runtime entry (`execution.rs:503-517`). [C] P1 requires the check "**Before** executing any part of the deployment" (P1:2191). [P]
- **Change.** Before execution, the producer and every validator compute the known demand $`\Delta^{\mathrm{known}}_s`$ of each signature lane $`s`$ from the envelope and $`R_0`$. A signature lane is the part of the demand that one signature pays. The known demand counts communications plus introduction, transfer and trace bytes. One analysis computes it as the demand side of the linear resource proof. The same pass decides whether the estimate is complete, which means that no dynamic logic can add cost. A complete estimate is held exactly, plus the fee, and no remainder is held. For an incomplete estimate, a construct that the analysis cannot prove contributes 0, and the unprovable remainder $`R`$ is held in full. Here $`\mathrm{phloLimit}`$ and $`\mathrm{phloPrice}`$ are the signed phlo limit and the signed phlo price of the envelope:

```math
R \;=\; \Bigl(\mathrm{phloLimit} - \sum_{s} \Delta^{\mathrm{known}}_s\Bigr) \cdot \mathrm{phloPrice}.
```

- **Rejection.** If the holds do not fit the supply at $`R_0`$, the candidate never enters the runtime. The rejection has no effect and no charge (DR-64 decision 5).
- **Only the client's demand.** The check holds only the client's demand, because P1's acceptance protocol checks only that demand (P1:2191-2200). A lane counts for the client when a source of the envelope may pay it. That is, a source is located at the purse of the lane, or a source permits the lane for at least one class. A signed permission sponsors a lane, and P2 checks one local proof per purse (P2:1520-1527). The demand of every other lane leaves the holds and makes the estimate incomplete. An example is the continuation of a funding slot, which deposits on the slot fund when it runs (P1:1534-1630). The user decided this rule on 2026-10-11, and DR-124 (the live check of the client's demand) records it.
- **Casper footprint.** None for the check itself. The user moved the code motion of G2 ahead of the live check. That step moves the single-offer play and replay code out of `runtime.rs` and `runtime_manager.rs` into cost-accounting code. Then the check lives entirely in cost-accounting code: the analysis, a new gate module and the offered execution path (`execution.rs:444-666`). Settlement keeps charging the measured use directly, so no hold floor is added. No message format changes. Legacy shards are unchanged.
- **Analyzers.** The check uses the static demand analyzer (`delta_sigma.rs`) and the lexical name resolver (`lexical.rs`). Both recursed at the commit `17e07307f`, so a deeply nested deploy could overflow a node's stack. Step G1-1 backported the iterative walks of the merge commit `29b729551` verbatim, with a citation. No branch had a metered or stack-safe resolver, so step G1-2 built one, in the style of the stack-safe pushdown automata (PDAs) of `feature/mettail`. Since the commit `8527aa6e2`, the helper `signature_lane` also marks a signature that holds a variable as dynamic. DR-122 (the linear cost estimate) records this as its item 11.
- **A defect in the resolver.** Before step G1-3, the resolver gave a name created inside a receive body the parent's randomness (`lexical.rs:53-56` at `17e07307f`). The runtime builds that name from the received data (`dispatch.rs:61-77`). Such names count as dynamic demand, so the held limit covers them. Step G1-3 fixed this defect, and DR-121 (names that a receive body creates are dynamic) records the fix. [C]
- **Effect on clients.** [I]
  - An offer with an incomplete estimate is accepted only if its sources can hold $`\mathrm{phloLimit} \cdot \mathrm{phloPrice} + 1`$ at $`R_0`$. Eligible sources hold the known demand and the fee. Signed sources that are not on-chain purses hold the remainder. Most real deploys call stored contracts, so most estimates are incomplete. Some offers that succeeded before step G1-6 are now rejected.
  - On-chain private-name purses hold nothing before execution. No run has shown their authority yet (`gate.rs:865-869`), and `authorize_wallets` already forbids them the fee (`direct_wallet_funding.rs:237-240`). This is rule G-5 of G1, a gap fill of its implementation map. So the gateway call now fails at the check (DR-124, §6.2). [C]
  - The estimate counts each communication once for each participant. A communication of a receive with $`n`$ binds has $`n + 1`$ participants. When they share a region, the estimate reaches $`n + 1`$ times the realized use in compute, transfer and trace. That is twice the realized use for a binary communication. An offer whose estimate exceeds its signed limit is rejected.
  - Clients such as Embers must size the phlo limit to their balances and to the estimate. The estimate depends only on the envelope and on public consensus inputs, so a client can compute it before signing.
  - An offer with several sources needs one more rule, which [Section 14 of the design document](cost-accounting-impl/linear-cost-estimate.md#14-the-client-sizing-rule) states and proves.
- **Fixtures.** [C] An incomplete estimate holds the whole signed limit. The user decided on 2026-10-10 to raise the funded test vaults to $`10^{12}`$. The fixture step of G1 did so (commit `419629883`). The hold of 200,000,001 that the shared helper signs now fits. The 64-source test needed no change. Its term is complete, so its holds land per class of charge on the one source of each class.
- **Verification.** `EndToEndAuthority.v` extends its branch lemmas to the held remainder (`EndToEndAuthority.v:612-665`). The test `an_underfunded_offer_never_enters_the_runtime_and_pays_no_fee` shows that an underfunded offer never enters the runtime (`offered_admission_api_test.rs:468`). [C]

### 4.2 G2: several offered deploys per block as an ordered batch executor (DR-65)

**Status: approved on 2026-10-10. Step G2-1 is committed (`72de6b52a`). Step G1-7 enforces item 5 of DR-65 for one offer per block (`88f4176db`). Step G2-2 and the later steps are not started.**

- **Problem.** A block holds at most one offer (`block_creator.rs:1910-1916`). Replay requires one isolated candidate (`offered_block.rs:784-788`). [C]
- **Change.** A block holds several offers in canonical order. Member $`i`$ funds from $`R_0`$ and executes from $`R_{i-1}`$, the root after member $`i - 1`$. Members that share a purse form one group. Let $`\mathrm{hold}_m(p)`$ be the hold of member $`m`$ on purse $`p`$, and let $`\mathrm{supply}_{R_0}(p)`$ be the balance of $`p`$ at $`R_0`$. If one purse $`p`$ of a group $`G`$ has $`\sum_{m \in G} \mathrm{hold}_m(p) > \mathrm{supply}_{R_0}(p)`$, every member of $`G`$ is rejected. This is P1's simultaneous-arrival rule (P1:2271-2287). A held-capacity guard keeps the holds of later members available.
- **Design choice.** The member loop is built as an ordered batch-executor API. The parallel executor of §7.10 can then parallelize it, and the executor of §7.6 can feed it finalized blocks. [I]
- **Casper footprint.** Step G2-1 moved the single-offer play and replay code into a cost-accounting module: `runtime.rs` +12/−289 and `runtime_manager.rs` +28/−554 (commit `72de6b52a`). The later steps still touch `replay_runtime.rs`, `block_creator.rs` and `errors.rs`. Their sizes need a new estimate. All changes sit behind the v6 seam.
- **Decided points.** [I]
  - Member $`i`$ is accepted at $`R_0`$ but reads its settlement inputs at $`R_{i-1}`$. Two offers of one signer share a cursor scope, so a read at $`R_0`$ makes the second settlement fail. This amends DR-65, and the user approved it on 2026-10-10. An amendment of 2026-10-12 records it in DR-65.
  - The guard is item 5 of DR-65 as adopted. Here $`\mathrm{balance}_{\mathrm{after}}(p)`$ is the balance of purse $`p`$ after member $`i`$ executes, and $`\mathrm{debit}_i(p)`$ is the debit of member $`i`$ on $`p`$, fee included. The guard requires $`\mathrm{balance}_{\mathrm{after}}(p) - \mathrm{debit}_i(p) \geq \sum_{j>i} \mathrm{hold}_j(p)`$. Settlement charges the measured use directly, so this one condition is exact. An earlier draft added a second condition, which only a settlement floor needed.
  - The design also caps the funding capacity of each member on a purse at the purse balance minus the later holds. The cap applies on the success path and on the rollback path, so a member never draws on funds that a later member holds.
- **Verification.** The planned model `SameBlockFundingGroups.tla` must check GroupAllOrNone, NoOverdraft, GateBeforeExecute, HeldCapacityGuard and DecisionPermutationInvariant. Negative controls must cover prefix admission and a missing held guard. Rocq must prove the three group lemmas of DR-65. `HeldCapacityGuard.v` already proves `held_capacity_guard_preserves_later_holds` without a settlement floor (`HeldCapacityGuard.v:67`). The other two lemmas and the TLA+ model remain.

### 4.3 G3: installer funding (DR-68)

**Status: approved on 2026-10-10, including the two changes to DR-68 that the decided points below describe. Not started.**

- **Problem.** Funding eligibility admits only the signed sources of the triggering envelope (`family_selection.rs:163-186` at the commit `17e07307f`, now `source_permits` at `family_selection.rs:89-120`). [C] Another owner's purse can be a source only in two existing ways: through a grant to a named delegate key, or as an on-chain private-name purse whose authority the run observes (`direct_wallet_funding.rs:289-300`). [C] So a continuation that an installer stored cannot draw on the installer's key wallet for an arbitrary caller (DR-68 context). The gateway flow fails today. The check before execution rejects it first, because rule G-5 gives its on-chain slot purses no hold (§6.2). [C]
- **Change.** The installer's located purse becomes an eligible source for obligations in the installer's stored signed region. A signed region is the part of a term under one signature $`s`$, as in $`\{P\}_s`$. Its seal is the stored record of that signature. When that budget is exhausted, the interaction does not fire and has no charge. This is the token gating of P1's rules.
- **Decided points.** [C][I]
  - "Does not fire" needs a veto. The accounting observer of RSpace has none (`rspace_interface.rs:116-122`), and a denied COMM aborts the deploy (`accounting/mod.rs:2079-2084`). RSpace already vetoes commits for `where` guards through `Match::check_commit` (`match.rs:34-39`, `space_matcher.rs:184-199`). That hook keeps the data and the continuation. Until a veto exists, the node rejects the triggering deploy with no charge. This interim rule deviates from DR-68, and the user approved it on 2026-10-10. The veto stays a tracked follow-up. The commit veto above puts the premise of this interim rule under review in G3 (audit of 2026-10-12).
  - `eval_cost_signed_term` (`rholang/src/rust/interpreter/reduce.rs:1920`) does not check the signer.
  - User syntax cannot create a key signature. Key regions come only from the deploy's own funding signature (`accounting/mod.rs:1359-1399`). `vault_payer` maps every non-Unit signature to a purse (`vault_payer.rs:45-103`). A key ground maps to its key wallet. A private name maps to its on-chain purse. Any other signature, including a compound of joint owners, maps to a purse keyed by its lane hash.
  - User syntax can still create spelled, quoted and held-name signatures. So G3 funds only seals whose signature locates a key wallet, and only seals that existed at $`R_0`$. This narrows DR-68, and the user approved it on 2026-10-10.
- **Casper footprint.** Cost-accounting code only. No dev Casper file changes.

### 4.4 The interim v6 merge rule (DR-120)

**Status: implemented and committed as `b496df9d8` on 2026-10-10.** Design v4 resolves every finding of the third independent check, and the user approved it on 2026-10-10.

**Purpose.** v6 shards merge until the architecture of §7 exists. The rule replaces dev's option search on v6 shards only. Legacy shards run dev's merger byte for byte. [I]

**Shape.** The rule comes from the deploy formats. A merge is a v6 merge when a parent or scope block other than genesis holds an offered user deploy. Validation already rejects mixed formats on v6 shards, so the rule needs no shard flag. The user chose this variant on 2026-10-10, because it removes three Casper files from the change. The rule has two paths below dev's dispatcher.

- **Fast path.** When the eight fast-path conditions F1 to F8 all hold, the rule composes all branches with `compute_merged_state` and rejects nothing. The table below states them. The code numbers its own checks differently. `fast.rs` runs the checks `F1` to `F13` and then composes as `F14` (`fast.rs:6, 145-245`). For example, condition F2 is the check `F3`, and conditions F4 to F7 are the checks `F7`, `F10`, `F11` and `F12`.
- **Slow path.** Otherwise one ordered pass replaces the option search.

| Condition | Meaning |
|---|---|
| F1 | The merge base is the main parent. |
| F2 | Every user deploy in scope is offered, and its committed evidence shows a charge. Its settlement therefore moves a cost cursor. |
| F3 | No chain in scope is late (window-closed). |
| F4 | No chain in scope conflicts with the event log of the base's lineage. |
| F5 | No two branches in scope conflict. The deploy-identity check runs on the raw chain list. |
| F6 | Dev's per-branch availability walk rejects nothing. The walk rejects each chain that consumes data that neither the base nor an earlier chain of its branch provides. |
| F7 | No channel fails the overfill predicate on the full set (`rholang_merging_logic.rs:237-267`), and no plain change mixes with a folded channel. |
| F8 | Every IntegerAdd channel stays within `i64` bounds. The sums use `i128` over per-chain diffs. |

The slow path is one pass in a strict total order $`K`$ on branches. The order puts settled (pinned) branches first. It then ranks branches with more prior losses first, then lower height, and then by dev's branch comparator (`conflict_set_merger.rs:40-81`).

The pass uses these names. `unavailable(b)` is the part of branch `b` that the availability walk rejects. `C[b]` is the set of branches that conflict with `b`. The purse ledger sums the per-chain diffs of each IntegerAdd channel. It admits `b` only if no purse balance goes below zero and no sum overflows. Pre-rejected chains are the chains that conflict with the base or with settled content before the pass starts.

```text
⟨ordered pass⟩ ≡
  rejected ← late chains, and every chain that depends on a late chain        -- dev rule
  for each branch b of the remaining chains:
      rejected ← rejected ∪ unavailable(b)                                    -- dev rule
  B ← the remaining branches, C ← their conflict map                          -- dev relation and deploy ids
  sort B by K(b) = (not pinned, −max loss, −sum loss, min height, compare_branches)
  for each b in B, in that order:
      keep b iff C[b] ∩ kept = ∅, no overfill and no folded mixing in kept ∪ {b},
             and the purse ledger admits b
  close the lineage over the rejected and pre-rejected blocks, pinned exempt  -- dev rule
  while the final ledger check or the overfill check fails:
      drop the last kept branch by K that contributes to the failure        -- unpinned first
      close the lineage again
```

A failure prefers a contributor whose diff has the failure's sign. If no such contributor exists, for example on a negative base, the pass drops a contributor of either sign. Each trim round therefore removes at least one branch, and the pass terminates. The composition folds the survivors in canonical order (`conflict_set_merger.rs:461-471`), so the result does not depend on hash-set order. [C][I]

**What it eliminates.** [I][M]

| Measure | Fast path | Slow path |
|---|---|---|
| Dev's Casper merge lines that do not run (of 4,477) | about 2,810 (63 %) | about 887 (20 %) |
| Net merge logic eliminated | about 55 % | not measured |
| CPU (central processing unit) time on the measured tests | It skips the adjudication. Dev's merge call took 1.0 to 1.6 % of test work. | One ordered pass replaces the option search. |
| What stays | the eager merge index, 7.8 to 10.5 % of test work | the same |

The line counts come from version 4 of the v6 merge-rule design. They are estimates of its inventory, which the repository does not hold, and not measurements of the committed code. The inventory counts non-blank, non-comment lines before the rule was committed. The 63 % counts dev lines that the fast path does not run. It is not the amount of logic removed. The arbiter of the next paragraph required this label as its change C8.

**Arbitration.** An independent arbiter checked version 2 of the design and accepted it with ten required changes, C1 to C10. Each change has a level. HIGH marks a defect that must close before implementation. MEDIUM marks a required change of the design, its proofs or its plan. LOW marks a precision fix. [I]

| ID | Level | Required change |
|---|---|---|
| C1 | HIGH | Add dev's folded-mixing rejection (`dag_merger.rs:438-462`) to F7 and to the pass. |
| C2 | HIGH | Run the deploy-identity check of F5 on the raw chain list. Chain equality uses only `deploys_with_cost` (`deploy_chain_index.rs:163-171`), so equal-cost copies collapse in a set. |
| C3 | MEDIUM | Define F8 and the ledger over per-chain diffs. |
| C4 | MEDIUM | Map availability-split errors to "the fast path fails" and to the rejection of that branch in the pass. |
| C5 | MEDIUM | Base the cursor-linearity lemma on the strictly rising cursor revision. |
| C6 to C10 | LOW | C6: exempt the approved genesis block from the format check. C7: list the exposed merge helpers. C8: label the 63 %. C9: fix one citation. C10: qualify the loss lemma. |

Design v3 resolves C1 to C10. A third independent check accepted v3 with the changes below. The label of each finding is its level and a number. [I]

| ID | Level | Required change |
|---|---|---|
| HIGH-1 | HIGH | The repair loop could stall on a negative IntegerAdd base, which a user can create. The fallback rule above closes it. A Rocq control, a TLA+ configuration and a unit test must cover it. |
| MEDIUM-1 | MEDIUM | Lineage closure removes single chains. A shrunken branch can then conflict with a kept branch. Check conflicts again after each removal. Dev has the same hole. |
| MEDIUM-2 | MEDIUM | The fast path must walk chains in the same order as the pass. Otherwise "fast path equals pass" fails on one numeric cell. |
| MEDIUM-3 | MEDIUM | Copy two dev helpers into the new module instead of moving them. Dev's `merge` function then stays almost unchanged. |
| MEDIUM-4 | MEDIUM | Thirteen Casper specs now run on an offered v6 genesis. They test the v6 rule, so the test plan must predict their outcomes. |
| LOW-1 to LOW-11 | LOW | Precision fixes. One matters to Casper readers: a fail-closed trip during validation is recorded as an invalid transaction, which is slashable. Valid parents cannot cause it. |

**Casper footprint.** The committed rule adds 104 lines and removes 13 in four dev files, plus a new module. [C]

| Dev file | Lines | Change |
|---|---|---|
| `interpreter_util.rs` | +26 / −1 | rule selection from the deploy formats, fast-path dispatch, the merge call |
| `dag_merger.rs` | +69 / −10 | `merge_with_rule`, a wrapper that keeps dev's signature, and crate visibility for two helpers |
| `conflict_set_merger.rs` | +8 / −2 | crate visibility for `compare_branches` and `branch_losses`. The formatter wraps each of the two signatures onto four lines. |
| `casper/src/rust/util/rholang/costacc/mod.rs` | +1 | register the new module |
| new `costacc/v6_merge/` | 1,527 lines, tests 2,599 lines | the rule |

The rule from the deploy formats removes three dev files from an earlier variant, which read a shard flag. System-only v6 merges use dev's merger, because they hold no offered user deploy. One cost remains on legacy shards: the rule reads the scope blocks before it selects dev's merger.

**Verification (committed).**
- **Rocq.** `V6MergeOrder.v`, `V6MergeLedger.v` and `V6MergeOrderedPass.v` prove order independence, conflict freedom, ledger validity, termination and fast-equals-slow. `CursorLinearity.v` and `CostCursorBuckets.v` cover the cursor arguments. Negative controls include an arrival-order tie-break and a missing claim check. Two more cover a set collapse that hides a duplicate and a sign-only repair that stalls on a negative base. The modules came with the commits `b496df9d8` and `7ab849b56`, with no admitted proof and no axiom.
- **TLA+.** `V6MergeSerialization.tla` checks three validators with permuted arrival orders. Eighteen failing configurations each refute one named invariant. `CursorDuplicateDetection.tla`, `FeeCursorBranchMerge.tla` and `FeeCursorCells.tla` cover the cursors and the lock fix.

**Same-signer sibling blocks: open for discussion.** [C][P][I] This is decision D-7, same-signer siblings, in §11.

- **The situation.** Two sibling blocks can each hold a fully funded offered deploy from the same signer. Each block settles its deploy against that signer's cost cursor at the same revision. The merge then sees two settlements that write the same cursor cells, so the two branches conflict.
- **What the interim rule does.** The ordered pass keeps the branch that comes first in the order $`K`$ and rejects the other branch.
  - The owner's node proposes the rejected deploy again, inside the deploy's validity window. This is the owner-only recovery of DR-116, the decision record on offered recovery.
  - The deploy then runs on the merged state and settles from the cursor that the winner left.
  - A rejected deploy that had failed leaves without a charge.
- **What P1 says.** Deployments with the same signature "are serialized by the linear proof" (P1:360-363). In sequence, the second deployment is accepted only if the pool still covers it (P1:2245-2269). So when the purse covers both deployments, P1 lets both succeed. Keep-one delays one of them, and it can drop a failed one without a charge.
- **Why both cannot simply survive.** A payer group is the set of payers that share one cost cursor.
  - For a payer group with several payers, the next cursor position depends on the allocation of the first settlement. The second settlement was planned from the old position. A merge cannot plan it again without executing it again.
  - For a single-payer group, both deployments could survive, but only with a redesigned cursor. It needs an additive revision counter and a position cell that the merge does not consume.
  - That redesign changes the cursor transition rules and the duplicate-detection argument. It revises at least six TLA+ models and three Rocq modules, and it takes about 3 to 5 engineer-weeks.
- **Under order-then-execute.** Both deployments execute in the canonical order, so the question disappears.
- **The question for discussion.** Accept keep-one until order-then-execute exists, or invest in the cursor redesign now.

### 4.5 The cost-cursor lock fix (bug 11004, DR-119)

**Status: implemented and committed as `7ab849b56` on 2026-10-10.** The only Casper source file that it changes is `SystemVault.rho`, with 88 lines added and 18 removed. It also restores dev lines in two dev specs, `exactly_once_spec.rs` and `verdict_convergence_spec.rs`. Each replaced line stays as a comment that states its reason.

- **Problem.** Before the fix, at the commit `b496df9d8`, `SystemVault.rho:52` declared one lock, and `:56` produced it once. `ensureCostCursor` consumed it on every call (`SystemVault.rho:145-167` at that commit). Every charged settlement therefore wrote one shared channel. [C] P1 requires that "RSpace is never locked by a single deployment" (P1:333-334). [P]
- **Effect.** Before DR-119, any two charged offered siblings conflicted in a merge, and a test relied on it (`offered_funded_api_test.rs:1858-1861` at the commit `17e07307f`). DR-119 migrated that test to two siblings of one signer. [C]
- **Change.** A settlement with an existing cursor takes no lock. A new cursor takes one of 256 bucket locks, keyed by the first byte of the Keccak-256 hash of its scope. The code checks again under the lock. Only cost-cursor code changes. The vault-creation code keeps dev's version, and its race is reported to the Casper team.
- **Residual false conflicts.** [I] A new payer cohort, which is a set of payers that share charge scopes, creates two cursors. Two new cohorts in one merge collide with a probability of about $`4/256 = 1/64`$. With $`m`$ new cohorts the expected number of colliding pairs is $`\binom{m}{2}/64`$. That is about 0.7 for $`m = 10`$, so the chance of at least one collision is about one half.
- **Order.** Without the v6 merge rule, sparse conflict graphs reach dev's exhaustive option search. The fix therefore lands second. [I]
- **Activation.** The change is in a genesis contract. It needs a fresh v6 genesis. A new legacy genesis gets the code but never calls it.

### 4.6 Footprint summary

| Item | Record | Casper files touched | v6 only | Status |
|---|---|---|---|---|
| G1 | DR-64 | none after the code motion of G2, cost-accounting code only | yes | committed: the live check (G1-6) and the guard for one offer per block (G1-7) |
| G2 | DR-65 | `runtime.rs` +12/−289 and `runtime_manager.rs` +28/−554 (G2-1). Later: `replay_runtime.rs`, `block_creator.rs`, `errors.rs`. | yes | G2-1 committed (`72de6b52a`). G2-2 onward not started. |
| G3 | DR-68 | none, cost-accounting code only | yes | approved, not started |
| v6 merge rule | DR-120 | 4 dev files, +104 / −13 lines, one new module | yes, by deploy format | committed (`b496df9d8`) |
| Cost-cursor lock fix | DR-119 | `SystemVault.rho`, cost-cursor code only | called on v6 only | committed (`7ab849b56`) |

---

## 5. Prior implementation on feature/casper-cost-accounting-completion

**Status: review finished on 2026-10-09.** At its tip `f9bd3895d`, the branch met none of the four expectations E1 to E4. It met E2 in part, but only inside one proposer's block, and only under a narrow reading in which funding decides. All `file:line` references in this section are at `f9bd3895d` unless the text names HEAD.

**Why this section exists.** The user reported that codex considered all four expectations met on the branch `feature/casper-cost-accounting-completion`. Codex is an automated coding agent. It made most of the Casper changes of that branch, in August and September 2026. A separate, read-only review tested the report against the code, the documents and the logged codex sessions. The review ran no code. It is a local artifact and is not in the repository.

**Verified facts.** [C]

| Fact | Evidence |
|---|---|
| The branch tip is `f9bd3895d` (2026-09-30, "Prepay native replay host work through cleanup and export"). | `git log` |
| The tip is an ancestor of HEAD. It is the first parent of the merge `1d325b996` (2026-10-01). | `git merge-base` |
| `1d325b996` merged dev `eb98d8e07` and states that it restored "dev's ordinary Casper protocol and consensus implementation". | the commit message of `1d325b996` |
| From `f9bd3895d` to `1d325b996`, the Casper merge module, `interpreter_util.rs` and `block_creator.rs` changed by 4,466 insertions and 8,254 deletions. | `git diff --shortstat` |
| At `f9bd3895d`, `acceptance.rs:2368` defines `admit_by_funding`, the gate of DR-11. No production code called it there. The proposer's gate was `certify_state_bound_admission` (`block_creator.rs:3022-3030`), which ran the state-bound loop. The commit `d367bc11a` (2026-08-16) made that switch. At HEAD neither function exists. | `git grep`, `git log -G` |
| At `f9bd3895d`, the multi-parent merge still ran dev's conflict search. `interpreter_util.rs:2756` calls `dag_merger::merge`, and `dag_merger.rs:2082` calls `conflict_set_merger::resolve_conflicts`, which is defined at `conflict_set_merger.rs:113`. | `git show` at `f9bd3895d` |
| Against its dev merge base, the branch changed its five merge files by 2,482 insertions and 1,726 deletions. It reworked the merger. It did not remove it. | `git diff --shortstat` |

**Why the realignment happened.** The user explained that the merge `1d325b996` was a necessary correction. The earlier branch had changed Casper beyond what cost accounting needs. The Casper team fixed the same problems in parallel, and the two versions conflicted. The review therefore separates changes that cost accounting requires from general Casper changes. Only the first kind can be proposed for reuse, and each one needs the user's approval.

### 5.1 Verdict for each expectation

| Expectation | What the branch did | Verdict | Key evidence at `f9bd3895d` |
|---|---|---|---|
| E1, merge logic | It reworked the merger into an exact-effect merge and removed no merge step. Non-test merge code grew by about 11 %, from 7,587 to 8,415 lines, against the dev merge base `375933475`. The count runs up to the first `#[cfg(test)]` in `casper/src/rust/merging`, `rspace++/src/rspace/merger` and `rholang/src/rust/interpreter/merging`. [C] | **NOT MET** under every reading | Proposal reaches `dag_merger::merge` through `block_creator.rs:3580`. Validation reaches it through `validation_dispatcher.rs:400` and `interpreter_util.rs:895`. In a branch test, two funded charges by different payers on one shared cursor scope conflict. The merge rejects one of them after both ran (`casper/tests/util/rholang/monetary_cursor_branches.rs:142,207,215-216`). [C] |
| E2, user alternatives | No mechanism lets a user pick the winner among alternatives. A user could already make alternatives exclusive by funding them from one purse or one funding slot. Compound signatures and the lollipop were native (`desugar.rs:7-13`, since the commit `3a4e03eb9`). Inside one proposal, a state-bound loop sorts the candidates canonically and admits a prefix of each funding group. [C] | **NOT MET.** Partly met only inside one proposer's block, under the reading "funding decides the competition". | Group closure in `runtime.rs:647,693,814,844,1116`. The prefix rule departs from the simultaneous-arrival rule of P1 (P1:2271-2287). Across sibling blocks, the merge decides after both blocks ran (`dag_merger.rs:104-121`, `conflict_set_merger.rs:353`). [C][P] |
| E3, keep accepting blocks | Nothing lets a node accept a block before it executes the block. The proposer executes each candidate in a scratch runtime before admission. [C] | **NOT MET** for blocks. The weak reading, no node-wide lock on RSpace, already held on dev. | The state-bound loop (`runtime.rs:627-1190`). Every validator recomputes the merge (`interpreter_util.rs:895`) and replays the block (`interpreter_util.rs:1073`) before the block is valid. Replays share one permit (`runtime_manager.rs:147`, acquired at `:1283-1287`). [C] |
| E4, performance | No change targets merge cost. The branch has merge timing instrumentation but no profile of merge cost (session `01a0e3fe`, line 10822). On v6 it adds merge and admission work. [C][I] | **NOT MET**, and not measured | Per-deploy exact state witnesses (`block_index.rs:196,273`), a projection check on every merge (`dag_merger.rs:1205-1211`), and the execution of every candidate before admission (`runtime.rs:627-1190`). [C] |

### 5.2 What the codex logs say

The review searched the prompt history of codex and its session logs from May to October 2026. [S]

- The review found no logged codex statement that all four expectations were met. The claim may exist outside the searched logs, for example in pull-request text.
- In the logged exchanges, codex rejected E1 and E4 and limited E3.
  - E1, on 2026-06-05: merge elimination "cannot mean all merge logic" (session `019e97d6`, line 718).
  - E1 and E2, on 2026-09-28: codex advised that the team "keep the residual multi-parent composition" and its conflict checks (session `01a0e3fe`, line 10252).
  - E3, on the same day: a node can receive or check more blocks while work runs. However, it "cannot certify a block’s final state root and settlement before that block’s execution/replay evidence is complete" (session `01a0e3fe`, line 10252).
  - E4, on the same day: codex found "no profile establishing that merge causes a majority" of the current latency. It added that the non-Casper campaign "will not, by itself, remove most existing merge cost" (session `01a0e3fe`, line 10822).
- The strongest "met" wording is in documents on the branch. [D]
  - DR-11 says that its gate "eliminates the run-to-completion lock and most merge analysis".
  - DR-15 calls its outcome "wholly (not partially) satisfied".
  - CA-P-171 and CA-P-172, two rows of the [conformance catalog](cost-accounting-conformance-properties.md), mark concurrent admission and conflict-free disjoint signatures as "COVERED".
  - Agent commits (prefix `[agent]`) wrote these texts in May and June 2026: `b34202b49`, `ae474e5b7` and `0f358af6b`. Codex edited DR-15 later (`a367ff27c`, 2026-08-31) and kept its outcome.
- The code at `f9bd3895d` does not support these texts. For example, the tests behind CA-P-171 call `admit_by_funding`, which has no production caller (`acceptance.rs:7935,8070`). [C]
- A codex post-mortem of 2026-10-01 (`b20e1bcc1`) states: "My earlier completion claims treated a real body-only accounting path as evidence that the newer offered-funded path was production-connected." It concerns production wiring, not E1 to E4. [D]

### 5.3 What the realignment removed, and why

On 2026-09-30 the user told codex to "replace all the casper changes in this branch with dev". The reason was that the branch must carry only cost accounting. [S] Codex first preserved the branch at `f9bd3895d`. The merge `1d325b996` then restored dev's Casper code on 2026-10-01. [S][C]

- After the merge, `casper/src` differed from dev `eb98d8e07` in 4 files, with 28 lines added and 28 removed. The differences are a recursion limit, one empty-state hash and test fixtures. [C]
- The removal was deliberate. It took out `acceptance.rs` (8,258 lines), every merge change in `casper/src/rust/merging/`, the state-bound admission, and the cost-accounting changes to genesis and contracts. [S][C]
- The review classified each of the 411 Casper files that the branch changed against its dev merge base `375933475`. A file counts as cost-accounting only if the review found no general edit in it. The classification is per file, so the borders inside mixed files are approximate. [C]

| Class | Meaning | Files | Lines |
|---|---|---:|---|
| Cost-accounting | Changes that cost accounting directly requires | 103 | +36,728 −1,796 |
| Mixed | Files with both cost-accounting and general changes | 39 | +29,509 −8,597 |
| General | General Casper fixes, for example of recovery, finality, fork choice, merge or validation | 269 | +51,854 −13,724 |

The cost-accounting code on the branch serves funding, settlement and replay. None of it removes merge work, lets a validator accept a block before execution, or measures performance. No cost-accounting code on the branch therefore meets E1, E3 or E4. [C][I]

### 5.4 Effect on this proposal

- No gap of §6 closes. [I]
- Sections 7 to 11 do not change. [I]
- One reference is worth keeping for E2: about 150 lines of in-block ordering and ledger logic. [C][I]
  - The parts are the canonical candidate order `canonical_sort` (`acceptance.rs:1761-1769`), the group closure of the state-bound loop (`runtime.rs:647,693,814,844,1116`), and the cross-group residual ledger `compute_settlement_debits` (`acceptance.rs:2273-2355`).
  - G2 (§4.2) can use them as a reference only after a change of the group rule, because their prefix rule departs from P1:2271-2287.
- The review advises against restoring the full body-only admission path of about 1,600 lines. It would add a second execution model beside the offered path of HEAD. [I]
- The only cost-accounting merge rule of the branch is DR-57, which makes a failed body with verified settlement a committed effect. HEAD already carries its cost-accounting part as DR-115. [D]

---

## 6. Remaining gaps

### 6.0 How to read the gap entries

Each entry has four parts:

1. **Paper.** The text that sets the expectation, quoted with line numbers.
2. **Code at HEAD.** What the code does now, with `file:line`.
3. **Why the interim work does not close the gap.** The interim work is §4.
4. **Recommended resolution.**

Each entry describes HEAD. The review of the earlier branch (§5) closes no entry.

Each gap has a name. GAP-E1 to GAP-E4 are the gaps against the expectations E1 to E4. GAP-i to GAP-iv are the gaps against the four items of the new model of P1 (P1:317-366):

- (i) explicit transaction boundaries (P1:318-325)
- (ii) concurrent acceptance (P1:327-334)
- (iii) throughput (P1:336-342)
- (iv) merge via signatures and cost accounting (P1:344-366)

### 6.1 GAP-E1: the merge logic remains

**Paper.** [P]
- "Merge analysis is expensive, complex, and a persistent source of bugs." (P1:300-301)
- "the analysis is *post-hoc*: deployments are executed speculatively, and conflicts are detected only after the work has been done." (P1:304-306)
- "The need for post-hoc merge analysis is substantially reduced." (P1:345-346)
- "Merging is no longer a separate analysis pass over execution traces" (P1:351-353).
- P1 says "substantially reduced", not "eliminated". The remainder is the shared-data residual (P1:363-366).

**Code at HEAD.** [C]
- The proposer merges before it plays (`interpreter_util.rs:932-942`).
- Every validator recomputes the merge and compares the records (`interpreter_util.rs:425-534`).
- Every validated block gets a merge index when the shard allows more than one parent (`validation_dispatcher.rs:340-375`).
- On v6 merges, one ordered pass selects survivors after execution (`ordered.rs:242-270`). Legacy merges keep dev's option search (`conflict_set_merger.rs:870-980`).
- At HEAD, `dag_merger.rs` and `conflict_set_merger.rs` differ from dev `93e38575e` by +103 and −34 lines. The v6 merge rule (`b496df9d8`) added most of them.

**Why the interim work does not close the gap.** [I] The v6 merge rule still reads execution traces. Its design inventory estimates that its fast path skips about 63 % of dev's merge lines. The repository does not hold that inventory. Its checks `F7` and `F10` to `F12` (`fast.rs:187-231`) are the conditions F4 to F7 of §4.4. They read the event logs and the changes of the chains in the block index. The eager index stays at 7.8 to 10.5 % of test work. Merging therefore remains "a separate analysis pass over execution traces", which P1:351-353 rules out.

**Recommended resolution.** Order-then-execute on v6 shards (§7). No state merge exists there. The executor's gate decides class S conflicts in the canonical order, and the canonical order serializes class D. The v6 merge rule retires on v6 shards in Phase 3, the order-then-execute phase of §10. Legacy shards keep dev's merger.

### 6.2 GAP-E2: users decide which alternatives they want

**Paper.** [P]
- Competing deployments "are two proof obligations competing for the same linear hypotheses, and at most one can succeed" (P1:348-351).
- Deployments with different signatures "cannot conflict" (P1:356-359). Deployments with the same signature are "serialized by the linear proof" (P1:360-363).
- Arrival in sequence: the first is accepted, the second is rejected before any step (P1:2245-2269). Simultaneous arrival: both are rejected (P1:2271-2287).
- A funding slot is "Open: anyone who deposits tokens on $`\mathit{slot}`$" (P1:1582-1583). The transition to self-funding "is a resource-exhaustion event, not a policy decision" (P1:1621-1622).
- Choice inside one deployment: acceptance holds the largest branch, and settlement refunds the rest (P1:2129-2152).

**Code at HEAD.** [C]
- Since the commit `e64261134`, play and every replay run the linear cost estimate before execution (`execution.rs:503`, `replay.rs:1642`). It holds only the client's demand, for the one offer of a block.
- The proposer selects at most one offer per block (`block_creator.rs:1910-1916`).
- Since the commit `7ab849b56`, an existing cursor takes no lock, and a new cursor takes one of 256 bucket locks (`SystemVault.rho:63-72, 186-229`). Siblings of different payers compose. Same-signer siblings still conflict (D-7).
- On v6 merges, one ordered pass picks the survivor after both deploys ran (`ordered.rs:242-270`). Legacy merges keep dev's option search (`conflict_set_merger.rs:870-980`).

**Why the interim work does not close the gap.** [I]
- G1 applies P1's gate to the one offer of a block today. G2, which is not yet built, will apply the arrival rules inside one block.
- Across validators, the v6 merge rule orders branches by $`K`$ after execution. It then drops the last branch by $`K`$ that would overdraw a balance (`ordered.rs:242-270, 344-395`). No arrival order plays a part.
- The interim keeps one of two fully funded same-signer siblings (decision D-7, same-signer siblings under (a), §11). P1:360-363 serializes them, and both can succeed when the purse funds both.

**How a user expresses alternatives under cost accounting.** [I]
- Funding defines the alternatives. A user funds one slot with enough for one deployment and points every alternative at that slot. The funding check runs per located surface (P1:586-591).
- Arrival order picks the winner (P1:2245-2269). Simultaneous arrival rejects both (P1:2271-2287).
- A choice by a condition belongs inside one deployment as branches (P1:2129-2152).
- No new envelope field is necessary.

**Consent that the code already supports.** [C] The code already lets several owners consent to one deployment. A compound signature needs every owner (P1:380-397). Each required owner sets a price ceiling and can veto a draw (`phlo_bounds.rs:31-50`). A certified minimax allocation sets the shares (`funding_minimax.rs`). These mechanisms decide funding inside one envelope. They do not order envelopes across validators.

**The funding-slot gap.** [C] At HEAD the slot mechanism above fails. Rule G-5 gives an on-chain slot purse no hold before execution (`gate.rs:865-869`, `direct_wallet_funding.rs:237-240`). The funding-slot test is a known failure. With the provisional caps, the check rejects the gateway call with "200001 atomic units of the remainder fit no source" (DR-124, §6.10). The resolution is pending (G3 and the G1 rule G-5).

**What remains unmet.** A user cannot rank the alternatives that the same user submits separately. The canonical order ranks them. Sub-question Q3 of decision D-0 (what Greg means, §11) asks who must pick the winner: the canonical order or the submitting user.

**Recommended resolution.** One acceptance order across validators: the canonical order of §7.5. The executor applies the gate in that order. Decisions D-5 (simultaneous arrival in a DAG) and D-6 (user alternatives) of §11 fix the details.

### 6.3 GAP-E3: keep running and keep accepting blocks

**Paper.** [P]
- Run-to-completion "accepts a deployment, executes it to termination ..., commits the resulting state changes, and only then accepts the next deployment" (P1:259-262).
- "No other deployment can be accepted, pattern-matched, or reduced while the current deployment is running." (P1:278-280)
- "RSpace can accept deployments *at any time* ... RSpace is never locked by a single deployment." (P1:327-334)
- "acceptance is a single, atomic decision that precedes all execution" (P1:2303-2304).
- Block validity becomes "a syntactic predicate" (P1:1711-1712).
- "A production validator runs persistently, re-drawing phlogiston from the wallet after each deployment." (P1:3753-3755)
- P1 is explicit for RSpace and for one validator. "Blocks" is Greg's extension (§2.5).

**Code at HEAD.** [C]
- A v6 block plays one envelope (`runtime.rs:346`), and the producer replays it (`offered_block.rs:315-323`).
- More than one envelope is an error (`runtime.rs:364-373`, `offered_block.rs:784-788`).
- The legacy loop plays each deploy to termination (`runtime.rs:455-476`).
- Validators merge and replay a block before they accept it (`interpreter_util.rs:402-557`).
- A node replays one legacy block at a time (`runtime_manager.rs:198-205, 1226-1229`). An offered replay takes no permit (`runtime_manager.rs:1317-1333`).
- Each native frontier forms one conflict component (`deterministic_reduction.rs:1068-1072, 1120-1122`). §3.7.4 gives its effect on E3 (added on 2026-10-13).

**Why the interim work does not close the gap.** [I]
- In execute-then-merge, the block content includes its post-state root (`CasperMessage.proto:597-605`). A validator can only check that root by execution. Acceptance of a block therefore waits for execution.
- A proposer needs the merged parent state before it can execute (`interpreter_util.rs:932-1007`).
- The v6 merge rule changes how the merge decides, not when it runs.
- A parallel executor shortens execution but does not decouple acceptance from it.

**Recommended resolution.** Order-then-execute (§7). Consensus accepts a block by a structural predicate. P1:1708-1714 calls validity a syntactic predicate that requires token backing. Under §7 the executor's gate checks that backing after finality. [I] Execution follows finality and never blocks consensus.

### 6.4 GAP-E4: the performance claim

**Paper.** [P]
- Throughput today is "bounded by the execution time of the slowest deployment in the queue" (P1:281-282).
- The new model admits "as many deployments as there are non-conflicting token supplies" (P1:341-342).
- P2 attributes the gain to "*protocol fusion*" of fuel acquisition into single transitions (P2:856-865).
- No paper measures F1r3node.

**Code and data at HEAD.** [M] §9 gives the data. In short, the merge dominated block cost in the August soak. A code comment records that the parents-post-state stage "dominates block cost in sustained-load soaks while the `dag.merge.*` buckets stay small" (`metrics_constants.rs:125-128`). In the August soak, settled-signature probes took 92 % of the merge call. After dev's fixes, the roots-lock wait stays high but does not separate failing sessions (`docs/ToDos.md:201-204`). Dev's analysis of a later soak, run 37343966570, points to the finality round length and to blocks that wait for missing parents (dev `74ab85dd2`).

**Why the interim work does not close the gap.** [I] The fast path skips the adjudication. The slow path replaces the option search with one ordered pass. Dev's merge call took 1.0 to 1.6 % of test work, and `resolve_conflicts` took 0.9 % in one profiled test (§9.1). The eager index and the reset, replay and checkpoint cycle stay.

**Recommended resolution.** Measure first (§9.3). Then build order-then-execute, which removes the most work and takes replay and lock waits off the consensus path.

### 6.5 GAP-i: explicit transaction boundaries

**Paper.** "The for-comprehension is the transaction primitive ... A deployment may contain multiple transactions" (P1:318-325). [P]

**Code at HEAD.** [C] The native meter charges each COMM (DR-32). Funding admits only the sources of the envelope itself (`family_selection.rs:89-120, 242-248`).

**Gap.** [C] P1's funding-slot form exists (`offered_gateway_slot_api_test.rs:492`), and a retained birth funds a later prepaid draw (`offered_prepaid_api_test.rs:175`). At HEAD the slot test fails, because rule G-5 gives an on-chain purse no hold before execution. No code lets a stored continuation draw on its installer's wallet.

**Recommended resolution.** Pending (G3 and the G1 rule G-5). G1 itself is live. No architecture change is necessary, and every candidate design needs the resolution.

### 6.6 GAP-ii: concurrent acceptance

**Paper.** "RSpace can accept deployments *at any time* ... Multiple deployments can be active simultaneously" (P1:327-334). [P]

**Code at HEAD.** [C] One offer per block (`block_creator.rs:1910-1916`). One legacy replay at a time, and an offered replay takes no permit (`runtime_manager.rs:198-205, 1317-1333`).

**Why the interim work does not close the gap.** [I] G2 accepts several offers in one block. Across blocks, acceptance still waits for the merge and the replay.

**Recommended resolution.** Order-then-execute for acceptance (§7). The parallel batch executor for execution (§7.10).

### 6.7 GAP-iii: throughput

**Paper.** "the F1R3Node can accept and begin executing deployments in parallel" (P1:338-339). [P]

**Code at HEAD.** [C] Play and replay are sequential (`runtime.rs:455-476`). A legacy replay holds the one replay permit (`runtime_manager.rs:1226-1229`), and an offered replay runs one isolated candidate (`offered_block.rs:784-788`).

**Why the interim work does not close the gap.** [I] G2 runs the members of a block one after another in canonical order.

**Recommended resolution.** The parallel batch executor (§7.10). Independent members run at once, and the record of the batch replays as a sequential execution in canonical order (amended on 2026-10-13).

<details>
<summary>Text before the amendment of 2026-10-13</summary>

Independent members run at once, and the result equals the sequential canonical execution.

</details>

### 6.8 GAP-iv: merge via signatures and cost accounting

**Paper.** "Merging is no longer a separate analysis pass over execution traces --- it is a consequence of the signature and cost-accounting structure of the calculus itself." (P1:351-354) [P]

**Code at HEAD.** [C] On v6 merges, one ordered pass selects survivors after execution (`ordered.rs:242-270`). Legacy merges keep dev's option search (`conflict_set_merger.rs:870-980`).

**Why the interim work does not close the gap.** [I] Inside a block, G1 and G2 decide conflicts by the linear proof. Across validators, the v6 merge rule still analyzes traces after execution.

**Recommended resolution.** Order-then-execute (§7). The gate decides class S in the canonical order. Class I composes, and with the parallel executor it also runs in parallel. The canonical order is the "natural serialization order" of class D (P1:363-366).

### 6.9 Gap matrix

The matrix rates each candidate design against the expectations and against the paper items (i) to (iv). [I]

| Design | E1 merge logic | E2 user alternatives | E3 keep accepting blocks | E4 performance | (i) transaction boundaries | (ii) concurrent acceptance | (iii) throughput | (iv) merge via signatures |
|---|---|---|---|---|---|---|---|---|
| (a) v6 merge rule, G1, G2 | Partial | Partial | Unmet | Marginal | Pending (G3 and the G1 rule G-5) | Partial | Unmet | Partial |
| (b) parallel batch executor, G1, G2, lock fix | Unmet | Partial | Partial | Partial | Pending (G3 and the G1 rule G-5) | In a block | In a block | In a block |
| (c) order-then-execute, sequential executor | Met on v6 | Met if Q3 picks the canonical order | Met | Largest, not measured | Pending (G3 and the G1 rule G-5) | Partial | Unmet | Met |
| (c)+(b) | Met on v6 | Met if Q3 picks the canonical order | Met | Largest, not measured | Pending (G3 and the G1 rule G-5) | Met | Met | Met |
| (d1) purse lanes | Partial | Met for funding | Unmet | Small | Pending (G3 and the G1 rule G-5) | Partial | Unmet | Partial |
| (d5) single-parent v6 | Mostly | Unmet | Unmet | Mixed | Pending (G3 and the G1 rule G-5) | Unmet | Unmet | Unmet |

### 6.10 Related gaps outside the four expectations

Two further gaps affect every design. Both must close before a v6 network carries production load.

**Host-work caps and the Embers initializer.** Embers is the F1R3FLY blockchain API that generates and deploys Rholang contracts ([real-world applications](../../rholang/19-real-world-applications.md)). The Embers testnet log initializer is a fixture in `offered_funded_api_test.rs:794`. Two tests run it. [M]

| Test | Committed caps | Provisional caps |
|---|---|---|
| `registry_initializer_is_funded_on_a_fresh_genesis` (`offered_funded_api_test.rs:987`) | Fails: "host work budget is rejected for verification operations: usage 20555630, requested 1" | Passes |
| `another_signer_initializes_a_registry_environment_after_a_registry_insert` (`offered_funded_api_test.rs:1025`) | Fails: "host work budget is rejected for verification operations: usage 20550481, requested 1" | Passes |

- **Source.** The test-suite runs of 2026-10-09 for work item D-F2. That item is the reuse of the reducer's measurement of each introduction (DR-118). The logs are `target/verification/d-f2/suite-committed.log` (lines 929-981) and `suite-provisional.log` (lines 933, 940). The suites of 2026-10-12 at HEAD show the same results (`target/verification/bug-11008/`). These local logs are not in the repository.
- **The two cap sets.** The committed caps are the protocol-6 host-work limits in `cost_protocol_limits.rs` and `production_limits.rs` at HEAD. The provisional caps are a candidate set of larger limits that the lane tests but has not committed. They change seven limit values in the same two files.
- **Limit of this evidence.** Both errors name the verification-operations dimension of the host-work budget. This document does not identify the smallest limit that the two tests need.
- **Why it matters for Casper.** The protocol-6 host-work caps are consensus constants. Play and replay must reject the same work. A shard must therefore fix them before activation.
- **Resolution.** Finish phase D of epic 8946, an internal phase of that epic. That work removes the last copies and repeated source builds from replay and play. Then measure the Embers workloads and settle the caps. Commit them with the activation of the v6 genesis.

**Settlement log-channel continuations.** [C][I] A vault owner can register a log channel (`SystemVault.rho:621-641`). Transfers, deposits and settlement steps send to it (`SystemVault.rho:571-580`, `MakeMint.rho:55-88, 130-174`). Settlement plays a system deploy (`settlement.rs:388-390`, `runtime.rs:1012-1029`). System deploys run as the system payer under `Cost::unsafe_max()` (`runtime.rs:1447-1459`). A user continuation that fires at settlement is therefore neither metered nor bounded.

Under the parallel executor, such a loop stalls a worker. Under order-then-execute it stalls the executor. Meter and bound these continuations before Phase 2 (the parallel executor) or Phase 3 (order-then-execute) of §10.

**Known test failures at HEAD.** [M] The suites of 2026-10-12 at HEAD have these known failures (`target/verification/bug-11008/`).

- Under both cap sets: the funding-slot gateway test (`offered_gateway_slot_api_test.rs:492`) and two legacy-charge tests. They are `replay_should_match_in_case_of_user_execution_error` and `compute_state_should_capture_rholang_parsing_errors_and_charge_for_parsing`.
- Under the committed caps also: `offered_replay_usage_is_identical_across_roles` and the two Embers tests above.
- Under the provisional caps also: the 64-source test.

The funding-slot failure is a gap for E2 and for (i) explicit transaction boundaries (§6.2, §6.5). With the provisional caps, the check before execution rejects the gateway call, because rule G-5 gives the on-chain slot purses no hold. With the committed caps, the test fails earlier, at its funding step. The resolution is pending (G3 and the G1 rule G-5).

---

## 7. The recommended architecture: order-then-execute for v6 shards

### 7.1 Overview

**Amendment (2026-10-13, record and replay).** The first version of §7 assumed a deterministic executor. Item 2 below now describes record and replay, and item 3 is new. The paragraphs after the list explain the change. [I]

Order-then-execute separates two jobs that Casper does together today. [I]

1. **Consensus orders.** Blocks carry signed envelopes that the proposer has not executed. Validators accept blocks by a structural predicate. Fork choice and finality work as today.
2. **Execution follows.** When the LFB advances, the linearizer orders the newly finalized blocks. One executor runs each block of that order. It applies P1's gate before each envelope, executes, settles, and publishes its record. The record is the event log and the evidence that play already produces.
3. **Replay checks.** The other nodes replay the record and attest the root. Replay follows the record, so honest replays of one record reach one root. A wrong attestation is shown by replaying its record.

<details>
<summary>Text before the amendment of 2026-10-13</summary>

2. **Execution follows.** When the LFB advances, a deterministic executor linearizes the newly finalized blocks. It applies P1's gate before each envelope, executes, settles, and records the root.

</details>

**Why the deterministic executor no longer fits.** [C][I]

- The first version required every honest executor to reach the same root from the same order. It said: "Execution is a deterministic function of genesis and the canonical order."
- The random candidate pick breaks that premise (§3.8). Two honest executors can match different candidates and reach different roots.
- The papers allow both. A run decides which interactions fire (P1:2090-2093), and the refund comes from the actual run (P1:2154-2160).
- The node already agrees in this way. The producer plays and records, and validators replay the record (§3.3, §3.4). Section 7 moves that pattern behind finality.

**What replay needs.** [C][I]

- *With the scheduler, nothing new.* Native replay already follows the recorded COMM (§3.8). Every replay of one record charges the same host work on one host and in one process (DR-79 item 4, DR-100, DR-102). Bugs 10814 and 10812 still separate hosts and processes, for today's replay too (§7.7). The restoration of the random pick adds RB-1 and RB-3a at the replay boundary (§3.8).

The design therefore works with the scheduler, and with a fixed or a random candidate pick. Consensus still orders the envelopes, and P1's gate still runs before each envelope (§7.6). [I]

This is the classic order-execute split of state-machine replication [Schneider 1990]. Calvin uses a deterministic order before execution for the same reason [Thomson et al. 2012]. Execute-then-merge resembles the execute-order-validate pattern of Hyperledger Fabric [Androulaki et al. 2018]. [I]

**Amendment (2026-10-13).** The state-machine approach assumes that replicas are deterministic [Schneider 1990], and Calvin assumes deterministic transactions. Section 7 no longer makes that assumption. One replica executes and records, and the others replay the record. [I]

Record and replay is an established technique for nondeterministic parallel programs. Instant Replay records the order of accesses to shared objects and forces that order in replay [LeBlanc and Mellor-Crummey 1987]. Eve handles nondeterministic replicas in another way. Its replicas execute first and compare their states. When the states differ, they fall back to sequential execution [Kapritsos et al. 2012]. [I]

![Sequence diagram of order-then-execute on v6 shards with record and replay, as amended on 2026-10-13. The participants are the signing user, the proposer, the validators, finality and the linearizer. They also include the executor of a block b, every other node, and a store of records and executed roots. In the inclusion phase, the user sends a signed offered envelope. The proposer reads the newest finalized block E whose canonical record it has replayed, and the root r_E. It runs the inclusion filter at r_E. Its block carries the envelopes, the attestation (E, r_E) and the epoch validator set. The block also carries the records that the proposer executed and that no earlier block cites. Validators run only structural checks, including the record encoding, and add the block to the DAG. A green note says that no step of inclusion or validation waits for execution or replay. In the finality phase, the LFB advances. The linearizer sends block b in the canonical order to the executor and to every other node. In the execution phase, the executor visits each envelope of b in block order and applies the P1 gate. An accepted envelope is held, executed and settled, and a rejected envelope has no effect. The executor then builds the record rec(b). It holds the pre-root, the gate verdicts, the processed deploys with their event logs and native cost evidence, the system work and the post-root. The executor certifies the record by one replay and cites it in its next block. An amber note says that the run is one valid run among many, and that the record fixes it. In the replay phase, a block that cites rec(b) is finalized. Every other node then fixes the canonical record of b. It is the first citation in the canonical order that starts at the root after the previous block and replays to its stated root. The node replays the record. If replay fails or ends at another root, the node records signed invalid-record evidence and tries the next citation. Otherwise the post-root becomes executed(b). In the attestation phase, a later block attests (E', r_E'). When replay reaches E', validators compare the attested root with executed(E') and re-run the inclusion filter. A red note says that a mismatch, an invalid record or a filter failure is signed slashing evidence. Any node proves it by replay, and it never changes block validity after the fact. A legend explains the colours. It defines P1, the P1 gate, the supply and the demand, DR-64, DR-65, E and r_E, b and e, and Ord and prev(b). It also defines rec(b), executed(b), canonical record, COMM, LMD-GHOST, LFB, DAG and v6.](diagrams/consensus-proposal-record-and-replay.svg)

*Source: [`diagrams/consensus-proposal-record-and-replay.puml`](diagrams/consensus-proposal-record-and-replay.puml). Render it with `./render.sh consensus-proposal-record-and-replay.puml` in `docs/casper/theory/diagrams/`.*

<details>
<summary>Diagram before the amendment of 2026-10-13</summary>

![Sequence diagram of the recommended order-then-execute pipeline for v6 shards. The participants are the signing user, the proposer, the inclusion filter, the validators, finality, the linearizer, the executor and a root store. In the inclusion phase, the signing user sends a signed offered envelope. The proposer reads its newest executed finalized block E and its root r_E. The inclusion filter applies the DR-64 check at r_E, the DR-65 group rule and a per-block demand cap. The proposer publishes a block with the envelopes, the attestation (E, r_E) and the epoch validator set. In the validation phase, validators run only structural checks and add the block to the DAG. A green note says that consensus keeps accepting blocks and that no step waits for execution. In the finality phase, fork choice and the clique oracle work as today. The linearizer orders the newly finalized blocks by main-chain epochs. In the execution phase, the executor visits each envelope in canonical order. It skips repeats and envelopes outside their window. It applies the P1 gate. An accepted envelope is held, executed and settled, and its fee goes to the including proposer. A rejected envelope has no state change and consumes no tokens. The executor runs the system work of each block and records the executed roots. In the attestation phase, the next block attests a newer root. Validators check the attestation when their own execution reaches that block. A red note says that a mismatch is signed slashing evidence and never changes block validity after the fact. A legend explains the colours. It also defines P1, the P1 gate, the supply and the demand of a signature lane, DR-64 and DR-65. It defines E and r_E, e, M with mp(M) and past(M), LMD-GHOST, LFB, DAG, v6 and CloseBlock.](diagrams/consensus-proposal-order-then-execute.svg)

*Source: [`diagrams/consensus-proposal-order-then-execute.puml`](diagrams/consensus-proposal-order-then-execute.puml). Render it with `./render.sh consensus-proposal-order-then-execute.puml` in `docs/casper/theory/diagrams/`.*

</details>

The whole design is one literate program with five parts. Sections 7.2 to 7.7 refine each part. The fifth part changed on 2026-10-13.

```text
⟨order-then-execute on a v6 shard⟩ ≡
  ⟨propose without execution⟩                 -- §7.2, §7.4
  ⟨validate a block structurally⟩             -- §7.3
  ⟨linearize the newly finalized blocks⟩      -- §7.5
  ⟨execute the canonical order⟩               -- §7.6: execute and record, or replay
  ⟨fix the canonical records, and check attestations when replay reaches them⟩   -- §7.7
```

<details>
<summary>Text before the amendment of 2026-10-13</summary>

```text
⟨order-then-execute on a v6 shard⟩ ≡
  ⟨propose without execution⟩                 -- §7.2, §7.4
  ⟨validate a block structurally⟩             -- §7.3
  ⟨linearize the newly finalized blocks⟩      -- §7.5
  ⟨execute the canonical order⟩               -- §7.6
  ⟨check attestations when execution reaches them⟩   -- §7.7
```

</details>

### 7.2 Block content

A v6 block keeps every consensus field of today: the sender, the sequence number, the signature, the justifications and the parents (`CasperMessage.proto:290-302, 329-334`). Its body changes meaning. [I]

| Field today | Meaning today | Meaning on a v6 shard under order-then-execute |
|---|---|---|
| `deploys` (`CasperMessage.proto:556`) | Executed deploys with their event logs | Accepted envelopes in block order, without event logs |
| `state.preStateHash`, `state.postStateHash` (`:598-599`) | The block's own merged pre-state and post-state | The attestation $`(E, r_E)`$: the newest finalized block $`E`$ whose canonical record the proposer has replayed, and the root $`r_E`$ after it (§7.7) |
| `state.bonds` (`:603`) | Bonds of the block's own post-state | The validator set of the block's epoch (§7.8) |
| `rejectedDeploys`, `appliedFromScope`, `mergeBase` (`:559-574`) | Merge results | Empty. Validity requires them to be empty. |
| `systemDeploys` (`:557`) | Executed system deploys | Slash records to execute at the block's position. CloseBlock work is implicit. |
| None today. A new field, which is a protocol-6 message change. | None | The cited records: the records that the proposer's executor produced for finalized blocks of its past cone, and that no earlier block cites (§7.7) |

**Amendment (2026-10-13).** The attestation row and the last row changed for record and replay. [I]

<details>
<summary>Text before the amendment of 2026-10-13</summary>

The attestation row read: "The attestation $`(E, r_E)`$: the newest finalized block $`E`$ that the proposer has executed, and its executed root $`r_E`$". The table had no row for records. The text after the table and the procedure read as follows.

The proposer does not execute. It reads its newest executed root and runs the inclusion filter (§7.4).

```text
⟨propose without execution⟩ ≡
  parents, justifications ← Casper fork choice and latest messages      -- unchanged
  (E, r_E) ← the newest finalized block that this node has executed, and its root
  V ← epoch_validator_set(parents)                                      -- §7.8
  batch ← ⟨inclusion filter⟩(pending envelopes in canonical order, r_E)
  publish block(parents, justifications, batch, attestation = (E, r_E), bonds = V)
```

</details>

The proposer does not execute while it proposes. It reads its newest replayed root and runs the inclusion filter (§7.4). It also attaches the records that its executor produced and that no block of its past cone cites (§7.6, §7.7).

```text
⟨propose without execution⟩ ≡
  parents, justifications ← Casper fork choice and latest messages      -- unchanged
  (E, r_E) ← the newest finalized block whose canonical record this node
             has replayed, and the root after it                       -- §7.7
  V ← epoch_validator_set(parents)                                      -- §7.8
  batch ← ⟨inclusion filter⟩(pending envelopes in canonical order, r_E)
  records ← the records that this node's executor produced,
            and that no block of past(parents) cites                   -- §7.7
  publish block(parents, justifications, batch, attestation = (E, r_E), bonds = V, records)
```

### 7.3 The validity predicate

A block's validity must be a pure function of the block and its past cone. It must not depend on how far the local executor has run. DR-44 is the record that honest parent selection preserves the committed LFB state. For the same reason, it rejected validity rules that depend on when a receiver learned finality. [I] The predicate therefore has two parts.

**Immediate checks.** They need no execution.

```text
⟨validate a block structurally⟩ ≡
  check the sender signature, sequence number, justifications and parents  -- unchanged
  check equivocation                                                       -- unchanged
  for each envelope e of the block:
      check the encoding and every signature of e
      check the validity window of e against the block number
      check e ∉ past(parents)                                              -- repeat rule
  check Σ phloLimit of the batch ≤ the per-block demand cap
  check bonds = epoch_validator_set(parents)        -- §7.8, can wait for local execution
                                                    -- (Q-6: how long may a node hold the block?)
  check that rejectedDeploys, appliedFromScope and mergeBase are empty
  for each cited record rec(b) of the block:                     -- §7.7, added on 2026-10-13
      check the encoding of rec(b), and check b ∈ past(parents)
  check that the cited records fit the per-block record cap       -- Q-7
  result: Valid or Invalid, with no merge, no execution and no replay
```

The two record checks are an addition of 2026-10-13. Validity does not replay a cited record. Replay of a cited record is a deferred check. [I]

The signed phlo limit bounds the demand of an envelope (DR-64). The demand cap is therefore a syntactic check. P1:1708-1714 calls validity a syntactic predicate that requires token backing. The demand cap does not check that backing. Under §7 the executor's gate checks it after finality. [I]

**Deferred checks.** They need the executed state at $`E`$. A node runs them when its replay of the canonical records reaches $`E`$ (§7.7, amended on 2026-10-13). The replay of each cited record is a deferred check too. A failure produces signed slashing evidence. It never invalidates a block after the fact.

<details>
<summary>Text before the amendment of 2026-10-13</summary>

A node runs them when its own executor reaches $`E`$ (§7.7).

</details>

### 7.4 The inclusion filter

The filter keeps blocks free of envelopes that cannot pay. It applies G1 and G2 at the attested root.

```text
⟨inclusion filter⟩(candidates, r_E) ≡
  batch ← empty list
  for each candidate e, in canonical order:
      skip e if e ∈ past(parents), or e is outside its validity window
      if the DR-64 check of e fails on its own sources at state(r_E):
          reject e locally, and go to the next candidate  -- no effect, no charge
      if adding e makes a DR-65 group overcommit a purse at state(r_E):
          reject the whole group locally, and go on       -- simultaneous arrival
      if Σ phloLimit(batch ∪ {e}) > per-block demand cap:
          stop
      append e to batch
  return batch
```

The filter is not binding. The executor's gate decides at execution, as P1 states (decision D-4, where acceptance binds, §11). [I] Two effects follow.

1. A concurrent block can spend a purse before the executor reaches a later envelope. The gate then rejects that envelope at no cost (P1:2208-2212).
2. Jointly overcommitted envelopes waste block space but cost their signers nothing. [I] Suppose that a block may carry at most $`k`$ envelopes that draw on one purse. Then each block adds at most $`k`$ failed envelopes for that purse.

**Option.** [I] The filter can also count the demand of the unexecuted envelopes in its past cone. That rule only filters and holds nothing. With it, only concurrent blocks can overcommit a purse. For $`n`$ concurrent proposers, at most $`(n - 1)\,k`$ envelopes of one purse then fail. Decision D-4 covers this option.

### 7.5 Deterministic linearization of finalized blocks

**The rule.** For a block $`L`$, let $`\mathrm{mc}(L) = (M_0, M_1, \ldots, M_k = L)`$ be its main chain from genesis $`M_0`$. Each $`M_{i-1}`$ is the main parent $`\mathrm{mp}(M_i)`$. The epoch of $`M_i`$ is the set of blocks that $`M_i`$ adds to the past cone. Let $`\mathrm{topo}`$ be a topological sort, by Kahn's algorithm [Kahn 1962], with ties broken by block number and then by block hash. Let $`\mathbin{+\!\!+}`$ concatenate sequences. The canonical order $`\mathrm{Ord}(L)`$ is then: [I]

```math
\mathrm{epoch}(M_i) = \mathrm{past}(M_i) \setminus \mathrm{past}(M_{i-1}),
\qquad
\mathrm{Ord}(L) = \mathrm{topo}(\mathrm{epoch}(M_1)) \mathbin{+\!\!+} \cdots \mathbin{+\!\!+} \mathrm{topo}(\mathrm{epoch}(M_k)).
```

Envelopes keep their block order. PHANTOM GHOSTDAG orders a block DAG along its selected-parent chain in the same way [Sompolinsky et al. 2021]. The word "epoch" in this rule means a main-chain step, not the staking epoch of §7.8.

**Properties.** [I] The Rocq development `CanonicalLinearization.v` must prove each property (§10).

1. *Partition.* $`\mathrm{past}(M_{i-1}) \subseteq \mathrm{past}(M_i)`$, because $`M_{i-1}`$ is a parent of $`M_i`$. The epochs are therefore disjoint, and their union is $`\mathrm{past}(L)`$ without genesis.
2. *Topological order.* Let $`X`$ be an ancestor of $`Y`$, with $`X \in \mathrm{epoch}(M_i)`$ and $`Y \in \mathrm{epoch}(M_j)`$. $`X`$ lies in $`\mathrm{past}(M_j)`$, so $`i \leq j`$. If $`i = j`$, $`\mathrm{topo}`$ puts $`X`$ first. If $`i < j`$, the concatenation puts $`X`$ first.
3. *Prefix stability.* If $`L_1 \in \mathrm{mc}(L_2)`$, then $`\mathrm{mc}(L_1)`$ is a prefix of $`\mathrm{mc}(L_2)`$. So $`\mathrm{Ord}(L_1)`$ is a prefix of $`\mathrm{Ord}(L_2)`$.
4. *Independence from finality steps.* Suppose a node executes along its LFB sequence $`L_1, L_2, \ldots`$ and each $`L_j \in \mathrm{mc}(L_{j+1})`$. By induction on property 3, it executes $`\mathrm{Ord}(L_{\mathrm{last}})`$ exactly. Two nodes with different LFB sequences therefore execute the same order.

**Why a simpler rule fails.** A rule that sorts each node-local finality batch by height and hash diverges. In the example below, $`M_1`$, $`M_2`$ and $`M_3`$ are main-chain blocks, and $`S_1`$ and $`S_2`$ are side blocks. The hash of a block $`X`$ is $`h(X)`$. Node A finalizes $`M_2`$ and then $`M_3`$, and node B finalizes $`M_3`$ in one step. With $`h(S_2) < h(M_2)`$, node B runs $`S_2`$ before $`M_2`$, and node A runs $`M_2`$ before $`S_2`$. The two executors reach different roots. [I]

![Diagram of the linearization by main-chain epochs. On the left is a block DAG. Genesis G has height 0. Main-chain block M1 and side block S1 have height 1. Main-chain block M2 has height 2, main parent M1 and secondary parent S1. Side block S2 has height 2 and main parent M1. The LFB M3 has height 3, main parent M2 and secondary parent S2. The green package shows the canonical order, and its title says that the main chain of the LFB defines the epochs. The epoch of M1 is {M1}, the epoch of M2 is {S1, M2}, and the epoch of M3 is {S2, M3}. Node A and node B both get the order M1, S1, M2, S2, M3. The amber package shows the rejected rule, which sorts each node-local finality batch by height and hash. Node A finalizes M2 and then M3, and gets M1, S1, M2, S2, M3. Node B finalizes M3 in one step. With h(M1) < h(S1) and h(S2) < h(M2), where h is the block hash, it gets M1, S1, S2, M2, M3. The two executors reach different roots. A legend explains the colours. It also defines the block names, LFB, DAG, past(X), the epoch of a main-chain block, the hash h(X) and the sort by height and hash.](diagrams/consensus-proposal-linearization.svg)

*Source: [`diagrams/consensus-proposal-linearization.puml`](diagrams/consensus-proposal-linearization.puml). Render it with `./render.sh consensus-proposal-linearization.puml` in `docs/casper/theory/diagrams/`.*

**Assumption A-LIN.** Every LFB lies on the main chain of every later LFB. [I] The clique oracle supports it. A visited block at or above the target's height "disagrees iff the target is not on its spine" (`clique_oracle.rs:269-278`). A finalized target therefore lies on the main chains of the agreeing validators.

The finalizer checks only that the state of the new LFB contains the settled effects of the current LFB (`floor.rs:615-638`). It does not check that the current LFB lies on the main chain of the new LFB. If A-LIN fails at one step, the executor holds at that LFB until a later LFB restores it. Question Q-1 asks the Casper team to confirm A-LIN.

```text
⟨linearize the newly finalized blocks⟩ ≡
  on each LFB advance from L_old to L_new:
      if L_old ∉ mc(L_new): hold, and wait for a later LFB      -- A-LIN, Q-1
      for each block M on mc(L_new) after L_old, oldest first:
          emit topo(past(M) \ past(mp(M)))                        -- ties: block number, hash
```

The finalizer already delivers the newly finalized set to an effect closure (`finalization_runner.rs:380-390`, `block_dag_key_value_storage.rs:1574-1640`). The linearizer hooks in there. [C][I]

### 7.6 The executor: the gate, execution and settlement

The executor runs the canonical order. For the envelopes of one block it uses the G2 batch executor of §4.2. [I]

**Amendment (2026-10-13).** Every node runs this executor. For each block, the executor either executes and records the block, or it replays the canonical record of the block (§7.7). The gate and the four rules below hold in both cases, because replay checks that the record follows them. [I]

```text
⟨execute the canonical order⟩ ≡
  for each emitted block b, in order:
      wait until the canonical record of b is fixed,
          or the executor convention lets this node execute b    -- §7.7
      if the canonical record of b is fixed:
          replay it from the current root                       -- replay follows the record
          executed(b) ← the root that the replay reaches
          continue with the next block
      R0 ← the current root
      groups ← DR-65 groups of the envelopes of b at R0          -- simultaneous arrival inside b
      for each envelope e of b, in block order:
          if e ran before, or e is outside its validity window:
              skip e                                            -- no effect
          else if the group of e overcommits a purse, or the P1 gate fails:
              reject e                                          -- no state change, no tokens
          else:
              hold, execute and settle e                        -- DR-64, DR-65 guard
              pay the fee to the sender of b
      run the slash records of b, then its CloseBlock work
      executed(b) ← the current root                           -- provisional until §7.7 fixes it
      rec(b) ← R0, the verdicts and processed deploys, the system work, executed(b)
      ⟨publish rec(b)⟩                                          -- §7.7
```

<details>
<summary>Text before the amendment of 2026-10-13</summary>

```text
⟨execute the canonical order⟩ ≡
  for each emitted block b, in order:
      R0 ← the current root
      groups ← DR-65 groups of the envelopes of b at R0          -- simultaneous arrival inside b
      for each envelope e of b, in block order:
          if e ran before, or e is outside its validity window:
              skip e                                            -- no effect
          else if the group of e overcommits a purse, or the P1 gate fails:
              reject e                                          -- no state change, no tokens
          else:
              hold, execute and settle e                        -- DR-64, DR-65 guard
              pay the fee to the sender of b
      run the slash records of b, then its CloseBlock work
      executed(b) ← the current root
```

</details>

Four rules come from the papers. [P]

1. The gate precedes all execution of an envelope. Acceptance "precedes all execution" (P1:2303-2304).
2. A gate failure has no effect: "No state change occurs. No tokens are consumed." (P1:2211-2212).
3. Arrival in sequence across blocks follows P1:2245-2269. The canonical order defines "in sequence".
4. Simultaneous arrival applies only inside one block (P1:2271-2287, decision D-5).

The executor keeps the user's rule against escrow, in the reading of this document. [I] Holds live only inside the execution of one block, as DR-65 designs for G2. Today the check holds the demand of one offer inside its block. The no-escrow review exempts short-lived reservations inside one lexical execution (`cost-accounting-impl/funding-settlement-design-review.md:305-315`). The user must confirm that a hold across the members of one block is such a reservation (decision D-4).

### 7.7 Records, replay and attestation of executed roots

**Amendment (2026-10-13, record and replay).** This section now defines the record of a block, the canonical record, replay, attestation and dispute. The text before the amendment follows in a `<details>` block at the end of the section. [I]

**The record.** The record of one run of a finalized block $`b`$ holds the items below. Play produces each item today, so the record needs no new kind of content. [C][I]

| Item | Content | Today's source |
|---|---|---|
| Pre-root | The root before $`b`$ | `R0` of §7.6 |
| Gate verdicts | Accepted, rejected or skipped, for each envelope of $`b`$ in block order | The gate of §7.6 |
| Processed deploys | For each accepted envelope: its cost, its event log, its error flag and its native cost evidence | `ProcessedDeployProto` (`CasperMessage.proto:523-530`) |
| System work | The processed slash and CloseBlock deploys of $`b`$, with their event logs | `ProcessedSystemDeployProto` (`CasperMessage.proto:548-552`) |
| Post-root | The root after $`b`$, as the executor states it | `executed(b)` of §7.6 |

**Notation.** The table defines the symbols of this section. [I]

| Symbol | Meaning |
|---|---|
| $`\mathrm{rec}`$ | One record of one block |
| $`\mathrm{pre}(\mathrm{rec})`$, $`\mathrm{post}(\mathrm{rec})`$ | The pre-root of the record, and the post-root that the record states |
| $`\mathrm{replay}(\mathrm{rec})`$ | The root that a replay of $`\mathrm{rec}`$ from $`\mathrm{pre}(\mathrm{rec})`$ reaches. It is $`\bot`$ when the replay fails. |
| Valid record | A record with $`\mathrm{replay}(\mathrm{rec}) = \mathrm{post}(\mathrm{rec})`$ |
| $`\mathrm{prev}(b)`$ | The block before $`b`$ in the canonical order $`\mathrm{Ord}`$ (§7.5) |
| $`G`$, $`r_G`$ | Genesis, and its root |
| $`\mathrm{rec}^{*}(b)`$, $`\mathrm{executed}(b)`$ | The canonical record of $`b`$, and the root after it |

**The canonical record.** The canonical record $`\mathrm{rec}^{*}(b)`$ of a finalized block $`b`$ is the first valid record of $`b`$ that starts at the root after $`\mathrm{prev}(b)`$. "First" follows the order $`\mathrm{Ord}`$ of the finalized blocks that cite a record of $`b`$. The definition recurses along $`\mathrm{Ord}`$: [I]

```math
\mathrm{executed}(G) = r_G,
\qquad
\mathrm{pre}\bigl(\mathrm{rec}^{*}(b)\bigr) = \mathrm{executed}\bigl(\mathrm{prev}(b)\bigr),
\qquad
\mathrm{executed}(b) = \mathrm{post}\bigl(\mathrm{rec}^{*}(b)\bigr) = \mathrm{replay}\bigl(\mathrm{rec}^{*}(b)\bigr).
```

```text
⟨fix the canonical record of b⟩ ≡                       -- on the finalized prefix only
  for each finalized block B that cites a record of b, in the order Ord:
      rec ← the record of b that B carries
      if pre(rec) ≠ executed(prev(b)):
          continue                                       -- built on another record: stale, no offense
      if replay(rec) ≠ post(rec):                        -- the replay fails, or ends at another root
          record signed invalid-record evidence against sender(B)
          continue
      return rec                                         -- the canonical record of b
  return none                                            -- wait for a later citation

⟨publish rec(b)⟩ ≡
  replay rec(b) once from its pre-root                   -- certification, as the producer does today
  carry rec(b) in this node's next block                 -- unless a block of its past cone cites b

⟨on a non-canonical own record⟩ ≡
  if this node executed b, and another record of b became canonical:
      drop its own results from b onward, and replay the canonical records
```

**Why honest nodes agree.** The argument has four steps. The model `OrderThenExecute.tla` of Phase 3 must check it. [I]

1. Honest nodes agree on $`\mathrm{Ord}`$ over the finalized blocks, by property 4 of §7.5 and assumption A-LIN.
2. A citing block carries its records. Every node that holds a finalized block therefore holds the same records. Record availability is block availability.
3. The replay of a record from its pre-root is a function of that record. With the scheduler, native replay follows the recorded COMMs and charges the same host work in every replay (DR-79 item 4, DR-100, DR-102). One known gap remains. Bugs 10814 and 10812 let host work depend on the host and the process (amendment of 2026-10-12 to DR-100). Today's validator replay has the same gap. It must close before activation, because the verdict at a cap boundary decides consensus (DR-100).
4. By induction along $`\mathrm{Ord}`$, honest nodes agree on $`\mathrm{executed}(\mathrm{prev}(b))`$. Steps 1 to 3 then give them the same first valid citation, so they agree on $`\mathrm{rec}^{*}(b)`$ and on $`\mathrm{executed}(b)`$.

**Who executes first.** [I] The binding rule above needs no designated executor. A convention avoids duplicate work, and consensus does not check it. The sender of $`b`$ executes $`b`$ first, as today's producer plays its own block. Other nodes wait a bounded local time for a citation, and then execute $`b`$ themselves. Question Q-7 asks the Casper team to choose this convention, the record cap and the retention of records.

**Attestation.** Each block also attests $`(E, r_E)`$: the newest finalized block whose canonical record its proposer has replayed, and the root after it. [I]

```text
⟨check attestations when replay reaches them⟩ ≡
  when this node fixes rec*(E) and sets executed(E):
      for each received block B with attestation (E, r):
          if r ≠ executed(E):
              record signed misattestation evidence against sender(B)
          if an envelope of B fails ⟨inclusion filter⟩ at state(r):
              record signed inclusion-rule evidence against sender(B)
```

**Dispute.** State is a function of genesis, the canonical order and the canonical records, and the canonical order fixes the canonical records. The replay of a record is a function of the record. Any node can therefore prove a wrong attestation, or an invalid record, by replay. [I]

- The evidence of a wrong attestation names the block $`B`$ and its attestation $`(E, r)`$. A checker replays the canonical records up to $`E`$ and compares the result with $`r`$.
- The evidence of an invalid record names the citing block and the record. A checker replays the record from its pre-root and compares the result with its post-root.
- Neither check needs a second execution, and neither depends on the order of a run.

Attestations serve three further purposes.

1. They let a node detect its own faulty replay. Honest replays of one record agree, so a large stake that attests another root is an alarm.
2. They give state sync and light clients a root that stake has signed.
3. They anchor the deferred inclusion check, because the filter must have used $`r`$.

The evidence class and the penalty of each offense are question Q-2.

**Residual points.** [I]

- The executor of a block picks one valid run among many, as today's producer does. Its choice of run can change the state that later gate verdicts read. Replay accepts any run that the rules allow (restoration plan, Q4 and Q5).
- The canonical record of a block is fixed only after a citing block is finalized. The attested root therefore lags the LFB by one execution and one finality round. §7.8 and the property BoundedLag must allow for that lag.
- Replay needs the records. Nodes must keep them, or a joining node must start from an attested root (Q-5, Q-7).

<details>
<summary>Text before the amendment of 2026-10-13</summary>

The section was titled "7.7 Attestation of executed roots" and read as follows.

Each block attests $`(E, r_E)`$: the newest finalized block that its proposer has executed, and the root after it. [I]

```text
⟨check attestations when execution reaches them⟩ ≡
  when this node's executor records executed(E):
      for each received block B with attestation (E, r):
          if r ≠ executed(E):
              record signed misattestation evidence against sender(B)
          if an envelope of B fails ⟨inclusion filter⟩ at state(r):
              record signed inclusion-rule evidence against sender(B)
```

Execution is a deterministic function of genesis and the canonical order. Any node can therefore prove a wrong attestation by re-execution. [I] Attestations serve three further purposes.

1. They let a node detect its own faulty executor. A large stake that attests another root is an alarm.
2. They give state sync and light clients a root that stake has signed.
3. They anchor the deferred inclusion check, because the filter must have used $`r`$.

The evidence class and the penalty of each offense are question Q-2.

</details>

### 7.8 Epoch-attested validator sets and the timing of slashing

**Today.** A block's bonds come from its own executed post-state (`CasperMessage.proto:603`, `block_metadata.rs:141-162`). The clique oracle reads them from the main parent (`clique_oracle.rs:202-220`). A slash zeroes a bond in the post-state of the block that executes it (slashing design, chapter 07). PoS already changes the active validator set at each epoch boundary (`PoS.rhox:558-589`). Block `bonds`, and so the finality weights, carry all bonds (`PoS.rhox:265-268`, `runtime.rs:1855-1862`). [C]

**Under order-then-execute.** Blocks have no own post-state, so the validator set comes from attested state. P1 updates validator sets at epoch boundaries. [P]

> "At each *epoch boundary* (when the set of staked validators is updated), the protocol mints a fresh supply of phlogiston for the validator's wallet, provided the validator has followed the rules of the protocol. If the validator commits a slashing offense, all remaining phlogiston is removed and no further phlogiston is minted." (P1:3036-3041)

The proposal: [I]

1. The validator set of epoch $`e`$ is the bond set in the executed state at the end of an earlier epoch. The lag in epochs is question Q-3.
2. Fork choice keeps its immediate exclusion of an equivocator. An equivocating block gets recorded evidence and the invalid-block effect (`validation_dispatcher.rs:611-633`). The estimator then filters the invalid latest message (`estimator.rs:3-23`). Detection is unchanged (`validation_dispatcher.rs:307-316`).
3. The clique oracle gets the same immediate exclusion: an equivocator in the target's view weighs zero (question Q-4). Today the bond becomes zero in the next post-state instead.
4. The slash record executes at its block's position. It removes the remaining phlogiston (P1:3040-3041). It moves the stake to adjudication (P1:3048-3050). Slash system deploys already carry a target activation epoch (`CasperMessage.proto:532-536`).
5. New bonds take effect at an epoch boundary.

A validator cannot check the `bonds` field of a block until its replay of the canonical records has reached the source epoch (amended on 2026-10-13). It holds such a block, as it holds a block with a missing dependency (question Q-6). The demand cap must keep the execution lag below one epoch. Record and replay adds the time until a citing block is finalized to that lag (§7.7). [I]

<details>
<summary>Text before the amendment of 2026-10-13</summary>

A validator cannot check the `bonds` field of a block until its executor has reached the source epoch.

</details>

### 7.9 What stays Casper, and why it remains Casper

The user set a rule for this work:

> "Don't make it a new protocol, it must remain Casper if at all possible. It must remain a secure, auditable, replayable, block-chain"

| Element | Under order-then-execute | Code |
|---|---|---|
| Signed blocks, sequence numbers, justifications | Unchanged | `CasperMessage.proto:290-302` |
| Several parents per block | Unchanged. They become ordering references, not merge inputs. | `CasperMessage.proto:329-334` |
| LMD-GHOST fork choice with the invalid-message filter | Unchanged | `estimator.rs:3-23` |
| Clique-oracle finality with the exact threshold | Unchanged decision rule. New weight source (§7.8). | `clique_oracle.rs` |
| Equivocation detection and slashing | Unchanged detection. The economic effect executes after finality. | `validation_dispatcher.rs:307-316` |
| State | A function of genesis, the canonical order and the canonical records, which the canonical order fixes (§7.7, amended on 2026-10-13) | new executor, and replay of the records |

The rule asks for four properties. [I]

- **Secure.** The CBC safety argument concerns agreement on the block DAG under justifications and the safety oracle. State comes from the agreed order and from the canonical records that the agreed order fixes (§7.7), so it does not weaken that argument. The model `OrderThenExecute.tla` of Phase 3 must check this claim. The validator-set source changes, and Q-3 and Q-4 cover that change.
- **Auditable.** Every block lists its envelopes. Every executed root is attested, and a replay of its records checks it. A wrong attestation is signed evidence.
- **Replayable.** Any node can replay the canonical records from genesis and reproduce every attested root. The record makes replay independent of the order and of the pick of any run (§7.7).
- **A block chain.** Blocks remain signed, hash-linked and finalized by Casper.

<details>
<summary>Text before the amendment of 2026-10-13</summary>

The table row "State" read: "A deterministic function of genesis and the canonical order", with the code "new executor".

- **Secure.** The executor computes state from the agreed order, so it does not weaken that argument.
- **Auditable.** Every executed root is attested and checkable.
- **Replayable.** Any node can replay the finalized order from genesis and reproduce every attested root.

</details>

The design therefore changes the interface between Casper and the replicated state machine. It does not change how Casper agrees on blocks [Schneider 1990]. [I]

### 7.10 The parallel batch executor inside a block

The executor of §7.6 can run the members of one batch in parallel. Its record must replay as a sequential execution of the members in canonical order (amended on 2026-10-13). [I]

<details>
<summary>Text before the amendment of 2026-10-13</summary>

The result must equal the sequential canonical execution. [I]

</details>

```text
⟨parallel execution of one ordered batch⟩ ≡
  ⟨group the members by shared purse⟩
  ⟨run the groups at once from the batch root⟩
  ⟨validate in canonical order and re-run conflicting members⟩
  ⟨combine the changes and settle⟩

⟨group the members by shared purse⟩ ≡
  groups ← union-find over the canonical custody keys      -- the DR-65 groups

⟨run the groups at once from the batch root⟩ ≡
  for each group g, at the same time:
      run the members of g in canonical order on a fresh runtime from R0
      record the read set and the write set of each member  -- event-log channels

⟨validate in canonical order and re-run conflicting members⟩ ≡
  for each member m, in canonical batch order:
      if the reads or writes of m meet the final writes of an earlier member of another group:
          run m again on the state that holds every earlier member
      the writes of m become final

⟨combine the changes and settle⟩ ≡
  root ← compute_merged_state(R0, the final changes in canonical order)
  settle each member exactly as the sequential executor does
```

**Basis.** Independent redexes commute (ST:367-383, ST:413-425). `DeterministicParallelReduction.v:115` proves that two non-conflicting intents commute in a two-channel model. Line 204 checks one fixed schedule by computation. Phase 2 must prove the batch theorem. Block-STM uses the same idea, with a preset order and a result that equals sequential execution [Gelashvili et al. 2023]. [P][C]

**Amendment (2026-10-13).** `DeterministicParallelReduction.v` belongs to the scheduler (§3.7). Its commutation theorem stays a valid basis for the batch theorem. Under record and replay, the batch theorem no longer needs one deterministic result. It needs only that the record of a parallel batch replays as a sequential batch in canonical order. Block-STM assumes deterministic transactions, and here the record of each member supplies that property. [I]

**Prerequisites.** [I]
- The cost-cursor lock fix (commit `7ab849b56`, done). Without it, every settlement writes one lock channel, and every pair of members conflicts.
- The metering of settlement log-channel continuations (§6.10).

Native scheduling inside one deploy is a separate source of parallelism. Today each native frontier forms one conflict component (`deterministic_reduction.rs:1068-1072, 1120-1122`). [C] §3.7 analyzes that scheduler.

### 7.11 Properties to verify

| Property | Kind | Meaning |
|---|---|---|
| ExecutedRootAgreement | safety | Two honest nodes that fix the canonical record of block $`b`$ set the same root $`\mathrm{executed}(b)`$. Amended on 2026-10-13. |
| ReplayAgreement | safety | Two honest replays of one record from one pre-root reach the same root and the same verdict. Added on 2026-10-13. |
| ProvableEvidence | safety | Any honest node can confirm, by replay, each misattestation and invalid-record evidence that an honest node records. Added on 2026-10-13. |
| GateBeforeExecute | safety | No step of an envelope runs before its gate accepts it. Replay checks this property in every record. |
| AtMostOneOfCompeting | safety | Of two envelopes that the purse cannot fund together, at most one settles. |
| NoMergeRejection | safety | A v6 block carries no rejected record. |
| ValidationNeverExecutes | safety | Structural validation runs no Rholang, and it replays no record. |
| BoundedLag | liveness | Under the demand cap, and while honest proposers keep proposing, every finalized block gets a canonical record within a bounded distance of the LFB. Amended on 2026-10-13. |

Failing TLA+ configurations must refute these properties when a guard is removed: ExecuteUnfinalizedOrder, LocalViewAuthoritative and NoDemandCap (§10). Since 2026-10-13, a further configuration, IndependentExecution, lets every node execute on its own with a random pick and share no record. It must refute ExecutedRootAgreement.

<details>
<summary>Text before the amendment of 2026-10-13</summary>

| Property | Kind | Meaning |
|---|---|---|
| ExecutedRootAgreement | safety | Two honest executors that reach block $`b`$ record the same root. |
| GateBeforeExecute | safety | No step of an envelope runs before its gate accepts it. |
| ValidationNeverExecutes | safety | Structural validation runs no Rholang. |
| BoundedLag | liveness | Under the demand cap, the executor stays within a bounded distance of the LFB. |

</details>

### 7.12 Alternatives considered

| Alternative | Why it was rejected or deferred |
|---|---|
| (a) Execute-then-merge with the v6 merge rule only | E3 stays unmet. E1 and E2 are only partly met (§6). It is the interim for v6 shards until Phase 3. |
| (b) Parallel batch executor inside execute-then-merge | It shortens play and replay. Consensus still waits for execution, and every merge cost stays. It is kept as Phase 2 and becomes part of (c). |
| (d1) Purse lanes | Per-purse proofs compose (P2:1520-1527). Data-channel conflicts between lanes still need a merge. One lane can be censored or can stall. Rejected. |
| (d5) Single-parent v6 shards | It skips the eager index (`validation_dispatcher.rs:345`). It drops the concurrency that merging recovers (P1:290-294). A sole parent that lacks the floor still re-bases through the merge (`interpreter_util.rs:1286-1321`). Rejected. |
| Optimistic execution on the fork-choice chain | Lower latency than executing only the finalized prefix. It needs rollback. Deferred (decision D-3, the execution point and the order rule, §11). |
| Holds bound at inclusion | A hold that survives across blocks needs a reservation table. The user prohibited "regression to the retired escrow-based model" (`funding-settlement-design-review.md:13`). Not recommended (decision D-4). |
| Pure height-then-hash order of finality batches | Nodes with different finality steps diverge (§7.5). Rejected. |
| A deterministic executor on every node, the premise of the first version of §7 | Every honest executor must then reach the same root. That needs a canonical candidate pick for executors, which would reverse the restoration of dev's random pick (§3.8). It also needs the single order of the scheduler (§3.7). Replaced by record and replay on 2026-10-13 (§7.1). |

### 7.13 Technical questions for the Casper team

These questions arise from the design. Each answer can change §7. [I]

| ID | Question |
|---|---|
| Q-1 | Does every LFB lie on the main chain of every later LFB (assumption A-LIN, §7.5)? |
| Q-2 | Which evidence class and which penalty apply to a wrong attestation, and to an inclusion-rule violation? |
| Q-3 | How many epochs of lag separate the attested bond state from the epoch that uses it? |
| Q-4 | May the clique oracle give an equivocator zero weight as soon as the target's view holds the equivocation? |
| Q-5 | How much attesting stake makes a root acceptable for state sync? |
| Q-6 | How long may a node hold a block whose `bonds` field it cannot yet check? |
| Q-7 | Which node executes each finalized block first, how large may the records in one block be, and how long must nodes keep records (§7.7)? Added on 2026-10-13. |

---

## 8. Change analysis

### 8.1 Areas that must change

"Scope" says whether a change affects only v6 shards or touches code that legacy shards share. Legacy shards keep dev's path in every row.

| # | Area | Files | Change | Cost-accounting reason | Risk | Interaction with ongoing Casper work | Scope |
|---|---|---|---|---|---|---|---|
| 1 | G1 check before execution | Cost-accounting code only (`gate.rs`, `execution.rs:321-345`, `replay.rs:1642`). Committed and live. | Run the DR-64 check before `evaluate_native_offered`. Validators recompute it. | P1:2188-2213 | Low | The stack-safe analyzer walks come from the F1R3Lang branches. | v6 only |
| 2 | G2 several offers per block | Code motion done in `72de6b52a` (`runtime.rs` +12/−289, `runtime_manager.rs` +28/−554). Remaining: `replay_runtime.rs`, `block_creator.rs`, `errors.rs`. | An ordered batch with DR-65 groups | P1:2271-2287, P1:327-334 | Low, a v6 seam | None known | v6 only |
| 3 | G3 installer funding | `family_selection.rs:89-120, 242-248` | The installer's purse is eligible for its stored region | P1:1557-1587 | Low | None known | v6 only |
| 4 | Cost-cursor lock fix | `SystemVault.rho:52-72, 186-229` | 256 bucket locks on cursor creation only. Done (commit `7ab849b56`). | P1:333-334 | Low. It needs a fresh v6 genesis. | The vault-creation race stays with the Casper team. | Genesis contract, called on v6 only |
| 5 | Interim v6 merge rule | `interpreter_util.rs`, `dag_merger.rs`, `conflict_set_merger.rs`, new `costacc/v6_merge` | Fast-path composition and one ordered pass. Done (commit `b496df9d8`). | P1:344-366 | Medium | Dev reworked the merger in August and September, for example `cdb85d1e4`. | Shared files, v6 behavior |
| 6 | Settlement log channels | `SystemVault.rho:571-580, 621-641`, `MakeMint.rho:55-174`, `runtime.rs:1447-1459` | Meter and bound continuations that fire at settlement | "every communicating process must engage the cost layer" (P1:2389-2391) | Medium | Dev's legacy precharge and refund likely share the hole. | Shared genesis contracts |
| 7 | Parallel batch executor | `runtime.rs`, `runtime_manager.rs`, `deterministic_reduction.rs` | Independent members at once, with a record that replays as the sequential batch | P1:327-342 | Low, the batch record replays as the sequential canonical batch | EPIC-021 measures the replay lock. It is the task-board epic on the root cause of issue #24, the finalization failures of the soak tests. | v6 only |
| 8 | Block body meaning | `CasperMessage.proto:554-575, 597-605` | Envelopes, the attestation, the epoch validator set and the cited records (§7.2) | P1:1708-1714 | High | Any work on the block format | Shared message, v6 meaning |
| 9 | Proposal without execution | `block_creator.rs`, `interpreter_util.rs:895-1032` | Build blocks from the inclusion filter, with no merge and no play | P1:2303-2304 | High | None known | v6 only |
| 10 | Validation without merge or replay | `validation_dispatcher.rs:84-334`, `interpreter_util.rs:402-557` | Structural validity and deferred attestation checks | E3, P1:1708-1714 | High | Overlaps EPIC-021, which measures replay and checkpoint waits | v6 only |
| 11 | Executor driven by finality | `finalization_runner.rs:351-560`, `block_dag_key_value_storage.rs:1574-1640`, `runtime_manager.rs` | Linearize each newly finalized set. Execute and record each block once, and replay the canonical records on every other node (§7.7). | P1:327-334 | High | EPIC-021 studies finalization latency. | Shared hook, v6 executor |
| 12 | LFB containment | `floor.rs:615-638` | DAG containment replaces state containment, because v6 has no merge rejections | P1:351-353, §6.1 | High, the finalized-floor proofs | The finalized-floor dossier | Shared, v6 branch |
| 13 | Validator set and slashing timing | `clique_oracle.rs:202-253`, `block_metadata.rs:141-162`, `estimator.rs`, PoS reads | Epoch-attested bonds, and equivocators weigh zero at once | P1:3036-3043 | High | Pull request (PR) #480 changed the finality weights (`clique_oracle.rs:222-253`). | Shared |
| 14 | State sync, deploy status, exploratory reads | node API, `casper/src/rust/engine/lfs_*_requester.rs` | Read from attested roots. Add the statuses included, finalized and executed. | Not in the papers. It follows from row 8. | Medium | API clients such as Embers | Shared |
| 15 | Recovery and the rejected-deploy buffer | `block_creator.rs`, DR-116 | Idle on v6, because there are no merge rejections | P1:351-353, §6.1 | Medium | None known | v6 only |
| 16 | Soak claims and models | `MergeAccounting.tla`, and the eight Casper soak claims CLAIM-CASPER-SOAK-001 to 008 in `docs/claims/` | New v6 profiles | Follows from rows 8 to 13 | High | The EPIC-021 soak harness | Shared |
| 17 | Host-work caps | `cost_protocol_limits.rs`, `production_limits.rs` | Settle the protocol-6 caps | Not in the papers. Replay must reject the same work (§6.10). | Medium | None known | v6 only |

**Amendment (2026-10-13).** Rows 7, 8 and 11 changed for record and replay (§7.2, §7.7). [I]

<details>
<summary>Text before the amendment of 2026-10-13</summary>

Row 7 read: change "Independent members at once, with the sequential result", risk "Low, the roots equal the sequential roots". Row 8 read: change "Envelopes, the attestation and the epoch validator set". Row 11 read: change "Linearize and execute each newly finalized set".

</details>

### 8.2 Change map

![Component diagram of the Casper change map for order-then-execute on v6 shards, read from left to right. Group 1 is the unchanged Casper core in indigo. It holds signed blocks and justifications, fork choice, equivocation detection and the clique-oracle decision rule. Group 2 is shared code that gets a v6 branch, in amber. It holds the block body for v6, the epoch validator set, the finality hook, LFB containment, and the API and state sync. Group 3 is new v6-only machinery in green. It holds proposal without execution, structural validation, the linearizer, the executor, the parallel batch executor and the attestation check. Group 4 holds the cost-accounting modules. G1, the cost-cursor lock fix and the v6 merge rule are committed, and G2 and G3 are designed. G1, G2, G3 and the cost-cursor lock fix are teal. The v6 merge rule is salmon, and it retires on v6 at Phase 3. Group 5 is dev's legacy path in grey. It holds the merger and the replay before acceptance. Each element carries its risk: high, medium or low. Arrows show the data flow from the core and the finality hook through the linearizer and the executor to the attestation and the block body. A legend explains the colours and the risk marks. It also defines v6, G1 to G3, DR-64, DR-120, pgmcp bug 11004, the P1 gate, Phase 3, LFB, LFS, API, DAG and LMD-GHOST.](diagrams/consensus-proposal-change-map.svg)

*Source: [`diagrams/consensus-proposal-change-map.puml`](diagrams/consensus-proposal-change-map.puml). Render it with `./render.sh consensus-proposal-change-map.puml` in `docs/casper/theory/diagrams/`.*

### 8.3 Interaction with ongoing Casper work

- **EPIC-021, the issue #24 root cause** (`docs/ToDos.md:74-106`). Order-then-execute removes the replay and the per-block reset and checkpoint from the consensus path of v6 shards. Legacy shards and the v6 executor keep the roots-lock fix `0ef0966c6`, which HEAD contains. The soak of that fix, run 37343966570, still had failing iterations. Dev's analysis found no per-node stage that separates them from passing ones (dev `74ab85dd2`). Soak comparisons must separate v6 and legacy profiles. [I]
- **Dev's task TASK-021-12.** Dev's task TASK-021-12 of EPIC-021, "Release parked blocks without queue, scan, and stale-link delays", changes how parked blocks wait for parents. Q-6 relies on that holding mechanism, so the two must compose. [I]
- **PR #480, the fix of issue 18** (`docs/ToDos.md:268`). It excludes silent bonded validators from the clique-oracle weight (`clique_oracle.rs:222-253`). Row 13 changes the weight source on v6 shards. The two changes must compose. [C][I]
- **Dev's merger rework.** For example `cdb85d1e4` resolves rejection groups independently. The v6 merge rule sits below dev's dispatcher and keeps dev's signature for its callers, so the conflict is medium. [C][I]
- **The finalized-floor and slashing dossiers.** Rows 12 and 13 change premises of mechanized proofs. Row 12 changes the state containment of the floor. Row 13 changes the bond-zero part of the fork-choice exclusion theorem T-10. Both dossiers need new proof obligations for v6. [I]
- **Epic 8946.** Its fixed exclusions authorize only the Casper edits that cost accounting requires. General consensus, finality and merge repairs stay with the Casper team. Order-then-execute changes consensus and finality, so it needs a new epic (decision D-9).

---

## 9. Performance analysis

### 9.1 Measured data and their limits

**Profile of two legacy multi-parent tests on dev.** [M] AMD uProf recorded hotspot profiles of dev `93e38575e` on 2026-10-09. The host was one AMD Ryzen Threadripper PRO 5975WX, pinned to 16 hardware threads. T1 is `hash_set_casper_should_compute_identical_post_states_across_validators_for_merge_blocks`. T3 is `a_co_witnessed_sibling_fork_must_adjudicate_and_advance`. The profiling plan also names the legacy test T2, `multi_parent_casper_should_allow_bonding`. Its profiles follow the table of the branch below. Rayon is the thread-pool library of the node, and its idle threads spin. "Work" is sampled CPU time minus that rayon idle spin.

| Inclusive share of work | T1 (work 9.30 s) | T3 (work 9.12 s) |
|---|---|---|
| Merge index, `block_index::new` | 10.5 % | 7.8 % |
| `compute_parents_post_state` | 1.2 % | 1.8 % |
| `resolve_conflicts` | not sampled | 0.9 % |
| Replay, `replay_compute_state` | 5.4 % | 7.5 % |
| Play, `compute_deploys_checkpoint` | 3.4 % | 4.9 % |
| Rholang `reduce` | 34.2 % | 27.9 % |
| RSpace produce and consume | 10.2 % | 7.8 % |
| Rayon idle spin, share of all sampled CPU | 63 % | 58 % |

Limits of this profile:
- The tests run about 10 s each. They are legacy tests, not v6 tests, and not a production node.
- Inclusive times overlap, so the shares do not add up.
- Much of the `reduce` time is genesis.
- The profiles of the branch at the commit `17e07307f` follow in the next paragraph.
- The raw reports are local artifacts and are not in the repository.

**Profiles of the branch at `17e07307f`.** [M] AMD uProf recorded T1 and T3 at the commit `17e07307f` on 2026-10-09, with the same host and pinning. It also recorded the offered test T4, `a_third_signer_inserts_a_version_after_a_merge_with_a_writers_branch`, at that commit. The profiles predate the v6 merge rule and the live check. The report step of each of these profiles needed 27 to 29 gigabytes of memory, so it ran under a cap of 32 gibibytes. The profiles of T2 finished later, and the paragraph after the table gives them.

| Inclusive share of work | T1, dev | T1, `17e07307f` | T3, dev | T3, `17e07307f` | T4, `17e07307f` (offered) |
|---|---|---|---|---|---|
| Work, sampled CPU minus rayon idle spin | 9.30 s | 11.19 s | 9.12 s | 10.50 s | 19.07 s |
| Merge index, `block_index::new` | 10.5 % | 10.2 % | 7.8 % | 7.0 % | 2.9 % |
| `compute_parents_post_state` | 1.2 % | — | 1.8 % | 1.4 % | — |
| `dag_merger::merge` | 1.1 % | — | 1.6 % | 1.0 % | — |
| Replay, `replay_compute_state` | 5.4 % | 4.6 % | 7.5 % | 6.1 % | 8.7 % |
| Offered certification, `certify_offered_draft` (self-replay and validator replay) | — | — | — | — | 7.2 % |
| Play, `compute_deploys_checkpoint` | 3.4 % | 2.7 % | 4.9 % | 3.7 % | — |
| Rholang reduction (dev `DebruijnInterpreter`, branch `ReducerCore`) | 34.2 % | 23.9 % | 27.9 % | 21.1 % | 34.0 % |
| RSpace produce and consume | 10.2 % | 7.6 % | 7.8 % | 7.6 % | 8.7 % |

A dash means that the report does not list the function. Either the function is below the report's cutoff of 400 entries, or it does not run on that path.

**T2 on dev and on the branch.** [M] Work grew from 10.24 s on dev to 12.64 s on the branch at `17e07307f`. The merge index took 11.6 % of work on dev and 10.8 % on the branch. Replay took 5.3 % and 5.2 %, play took 2.3 % and 1.9 %, and Rholang reduction took 35.5 % and 24.2 %.

What the profiles of the branch show: [M][I]
- The merge takes about the same share of work on the branch as on dev. In the offered-deploy test its share is smaller.
- The branch does 15 to 23 % more work than dev on the three legacy tests. The extra work is outside the merge.
- In the offered-deploy test, replay takes 8.7 % of the work. Replay contains each validator's call of `certify_offered_draft`. So replay and the producer's self-replay together take between 8.7 % and 15.9 %.
- These are short tests. They do not decide E4, the performance claim, for a loaded network. The three-validator v6 soak that §9.3 lists is still needed.

**August soak, GitHub Actions run 33099406770.** [M] One merge call took 2.16 s on average. The settled-signature probes took 92 % of it. The walk depth grew with the finalization lag, which deepened the lag further (`docs/claims/settled-effect-probe-equivalence.md:27-33`). Dev then batched the probes in `c9aff7732`, which HEAD contains.

**October soak, GitHub Actions run 37224478325.** [M] The run used `95be0d450` of `master`, the release branch, on a legacy shard. The user cancelled it in segment 3 (`docs/ToDos.md:105`).

| Metric, sustained phase | Passing sessions | Failing sessions | Source |
|---|---|---|---|
| Sessions | 76 | 22 | `docs/ToDos.md:197` |
| Finalization p95 (95th-percentile latency), median (gate 45 s) | 46.9 s | 58.7 s | `docs/ToDos.md:200` |
| Roots-lock wait per validator, median | 7.1 s | 9.1 s | `docs/ToDos.md:201` |
| Checkpoint time | 79 ms | 116 ms | `docs/ToDos.md:241` |
| Replay runtime-lock wait | 140 ms | 215 ms | `docs/ToDos.md:246` |

No metric separated failing sessions from passing sessions well. The AUC was at most 0.70 for any metric (`docs/ToDos.md:202`). Dev's roots-lock fix `0ef0966c6` is in HEAD. This branch records no verdict for its soak, run 37343966570. Dev recorded one on 2026-10-06 (dev `74ab85dd2`, issue #24 comment 6016872961). No per-node stage separated failing from passing iterations. Two dev changes confound later comparisons (`docs/ToDos.md:267-269`). They are PR #480 and PR #523, which stops in-flight marker leaks in the block processor.

**Branch against dev.** [M] The integrated design review reported a local timing of the branch tests against dev. The branch was slower by a factor of 1.13 on merge-heavy tests and 1.15 on the rest. The slowdown was not in the merger. This document could not locate the artifacts of that timing, so the numbers are unverified.

**Reading.** [I] The merge dominated block cost in August. The stage metrics record the same observation: the parents-post-state stage "dominates block cost in sustained-load soaks while the `dag.merge.*` buckets stay small" (`metrics_constants.rs:125-128`). After dev's fixes, the roots-lock wait stays high but does not separate failing sessions (`docs/ToDos.md:201-204`). Dev's analysis of soak 37343966570 points to the finality round length and to blocks that wait for missing parents (dev `74ab85dd2`). The merge, the per-block reset, the replay and the checkpoint all come from execute-then-merge with run-to-completion.

### 9.2 What each design and phase removes

| Design | Phase | Removes | Keeps |
|---|---|---|---|
| (a) | Phase 1b, interim merge (§10) | The fast path skips the adjudication and the settled-signature probes. The slow path replaces the option search with one ordered pass. Dev's merge call took 1.0 to 1.6 % of test work. | The index, 7.8 to 10.5 %. The reset, replay and checkpoint cycle. The v6 self-replay. |
| (b) | Phase 2 | Wall time of play and replay for independent members, up to 8.8 to 12.4 % of test work. CPU time stays. | Every merge cost. Consensus still waits for execution. |
| (c) | Phase 3 | The index and the parents post-state, about 10 to 12 % of test work (11.7 % for T1, 9.6 % for T3). Executions of deploys that the merge would reject. Replay and lock waits leave the consensus path. | `reduce`, 27.9 to 34.2 %, much of it genesis. RSpace operations, 7.8 to 10.2 %. Rayon idle spin. The certifying replay of the executor of each block, and one replay of each record on every other node, as today, but after finality (§7.7). |

**Amendment (2026-10-13).** Under record and replay, the executor of a block certifies its record by a replay, as today's producer does (§7.7). The (c) row therefore no longer lists the v6 self-replay as removed work. It lists the replays as kept work, off the consensus path. [I]

<details>
<summary>Text before the amendment of 2026-10-13</summary>

The (c) row listed "The v6 self-replay." among the removed work. Its kept work ended with "Rayon idle spin."

</details>

The play share contains the proposer's merge (`interpreter_util.rs:932-942`), so the play and merge shares overlap. [C] No design touches the rayon idle spin, 58 to 63 % of sampled CPU. [M] Order-then-execute also ends the August feedback loop. Consensus no longer merges, so a finality lag cannot slow a merge. [I]

### 9.3 Measurements still needed, and the decision rule

| ID | Measurement |
|---|---|
| M1 | AMD uProf reports for the branch tests T1 to T3 and for the offered test T4, `a_third_signer_inserts_a_version_after_a_merge_with_a_writers_branch`, one at a time. Done for T1 to T4 (§9.1). |
| M2 | A three-validator v6 soak with the stage histograms (`metrics_constants.rs:125-165`) and lock counters, against a dev legacy soak |
| M3 | Validation time with and without the checkpoint and replay stages |
| M4 | Executions per deploy (play, self-replay, replays), and executions of deploys that a merge later rejects |
| M5 | The share of each block's envelopes with disjoint funding footprints |

Let $`t_{\mathrm{merge}}`$ be the merge-stage time, $`t_{\mathrm{replay}}`$ the replay time and $`t_{\mathrm{wait}}`$ the lock-wait time of one v6 block. Let $`t_{\mathrm{block}}`$ be its total processing time.

```math
\rho \;=\; \frac{t_{\mathrm{merge}} + t_{\mathrm{replay}} + t_{\mathrm{wait}}}{t_{\mathrm{block}}},
\qquad
\rho > \tfrac{1}{2} \;\Rightarrow\; \text{E4 is confirmed},
\qquad
\rho < \tfrac{1}{5} \;\Rightarrow\; \text{E4 is refuted for the current code}.
```

Under the strict readings of decision D-0 (§11), E1 and E3 require order-then-execute whatever $`\rho`$ turns out to be. E2 also needs it if Greg means that the canonical order picks the winner among alternatives. [I]

---

## 10. Phased plan

The plan has five phases: Phase 0 (evidence), Phase 1 (acceptance core), Phase 1b (interim merge), Phase 2 (parallel executor) and Phase 3 (order-then-execute). In this document "P1" always names the paper, never a phase. Sizes are relative estimates: S (small), M (medium), L (large) and XL (extra large). Only Phase 0 and Phase 3 carry a duration estimate.

![Activity diagram of the phased plan. Three branches start in parallel. Phase 0, evidence, size S, covers the measurements M1 to M5 and gives evidence for or against E4 performance. Phase 1, acceptance core, size L, covers G1, G2 and G3. G1 is committed and live. Of G2 only the code motion is committed, and G3 is not started. Inside one block it meets (i) transaction boundaries, the P1 gate, E2 user alternatives and (iv) merge via signatures. Phase 1b, interim merge, size M to L, is committed. It adds the v6 merge rule and then the cost-cursor lock fix, and it meets E1 merge logic in part. After the join, a prerequisite fix meters and bounds settlement log-channel continuations. A decision point follows, for the user, Greg and the Casper team. It asks if keep accepting blocks means acceptance before execution. The no branch is option B of decision D-1, the target for v6 shards. On it, Phase 2, parallel batch executor, size L, meets (ii), (iii) and (iv) inside one block, and E3 stays unmet. The recommended yes branch is option A of D-1. On it, Phase 2 builds the same executor, and Phase 3, order-then-execute, follows. Phase 3 is size XL, about four to nine engineer-months. It meets E1, E3 and E4 on v6 shards, and E2 if Q3 picks the canonical order. Each phase box names its verification artifacts. A legend explains the colours and defines every name that the boxes use.](diagrams/consensus-proposal-phase-plan.svg)

*Source: [`diagrams/consensus-proposal-phase-plan.puml`](diagrams/consensus-proposal-phase-plan.puml). Render it with `./render.sh consensus-proposal-phase-plan.puml` in `docs/casper/theory/diagrams/`.*

Every Rocq proof is axiom-free and has negative controls. Every TLA+ model has failing configurations that must refute their invariants. Every invariant becomes a property test.

| Phase | Content | Size | Prerequisites | Verification | Meets |
|---|---|---|---|---|---|
| Phase 0, evidence | M1 to M5 (§9.3) | S, 1 to 2 weeks | None | Preregistered experiments with A/A calibration | Evidence for or against E4 |
| Phase 1, acceptance core | G1 with the DR-64 holds, G2 as a batch executor with DR-65 groups and the held-capacity guard, G3. Status: G1 committed and live. Of G2, only the code motion is committed. G3 is not started. | L | The G1 chain of epic 8946 | `SameBlockFundingGroups.tla`: GroupAllOrNone, NoOverdraft, GateBeforeExecute, HeldCapacityGuard, DecisionPermutationInvariant. Controls: prefix admission, no held guard. Rocq group lemmas. `EndToEndAuthority.v` extensions. `HeldCapacityGuard.v` (committed). | (i) transaction boundaries, the gate of P1, E2 user alternatives and (iv) merge via signatures, inside one block |
| Phase 1b, interim merge | The v6 merge rule with C1 to C10, then the cost-cursor lock fix. Status: committed, with its proofs and models (`b496df9d8`, `7ab849b56`). | M to L | None | `V6MergeOrderedPass.v`, `CursorLinearity.v`, `CostCursorBuckets.v`, `V6MergeSerialization.tla`, `FeeCursorBranchMerge.tla`, `FeeCursorCells.tla`, with the C1 and C2 controls | E1 in part, bounded merges on v6 |
| Phase 2, parallel executor | The parallel batch executor | L | Phase 1, the log-channel fix, and Phase 1b or Phase 3 | Rocq: the record of a parallel batch replays as the sequential canonical batch. Controls: a shared-purse pair in parallel overdraws, an arrival-order commit is not canonical, a skipped re-run misses a COMM. TLA+ `ParallelBatchExecutor.tla` with SequentialEquivalence, NoOverdraft, HeldCapacityGuard, RecordReplayRoot, and failing NoRevalidation, AuthorityKeyIgnored, NoHeldGuard. 256-seed property tests against a sequential replay of the batch record. | (ii) concurrent acceptance, (iii) throughput and (iv) merge via signatures, inside one block |
| Phase 3, order-then-execute | §7 | XL, 4 to 9 engineer-months | Phase 1, the log-channel fix, the decisions of §11, a new epic | TLA+ `OrderThenExecute.tla`: three validators, permuted arrival orders, finality as an oracle, the invariants of §7.11, liveness BoundedLag, failing ExecuteUnfinalizedOrder, LocalViewAuthoritative, NoDemandCap, IndependentExecution. Rocq `CanonicalLinearization.v` (order deterministic and stable under prefix extension) and `OrderedGateSoundness.v` (the sequential gate is sound per purse, P2:1520-1527). Controls: arrival-order blocks diverge, a gate after execution strands funds, independent execution under a random pick diverges. Three-node tests and a v6 soak. | E1, E3, E4 on v6 shards, and E2 if Q3 picks the canonical order |

**Amendment (2026-10-13).** The verification of Phase 2 and Phase 3 changed for record and replay (§7.7, §7.11). [I]

<details>
<summary>Text before the amendment of 2026-10-13</summary>

Phase 2 read: "Rocq: a parallel batch equals the sequential canonical batch", the TLA+ invariant "DeterministicRoot", and "256-seed property tests against a sequential reference". Phase 3 had neither the failing configuration IndependentExecution nor the control "independent execution under a random pick diverges".

</details>

The G2 member loop is the seam between the phases. Phase 1 builds it as an ordered batch API, Phase 2 parallelizes it, and Phase 3 feeds it finalized blocks. G1 stays in every phase. Under Phase 3 it becomes the inclusion filter and the executor's gate. [I]

The protocol-6 host-work caps of §6.10 must be settled before any v6 activation, whichever phase ships first.

---

## 11. Open decisions

These decisions belong to the user, Greg and the Casper team. Each row gives the options, the recommendation of this document, and what each option leaves unmet. [I]

| ID | Question | Options | Recommendation | What each option leaves unmet |
|---|---|---|---|---|
| D-0 | What does Greg mean? | Q1: does "keep accepting blocks" mean that consensus accepts a block before it executes it? Q2: does E1 count merge code that no longer runs, or the removal of after-the-fact trace analysis? Q3: who picks the winner among alternatives that a user submits separately, the canonical order or that submitting user? | Ask Greg first. | If Q1 is yes, only (c) among the designs of §6.9 meets E3. If Q1 is no, (b) is enough. If Q2 counts lines, (a) is enough for E1. If Q3 is "the submitting user", no paper mechanism exists (option B of D-6). |
| D-1 | The target for v6 shards | A: (c)+(b). B: (a)+(b), the v6 merge rule with the parallel batch executor. C: (a) alone. | A. It meets E1, E3 and E4, and E2 if Q3 picks the canonical order. It is XL and changes Casper's core. | B leaves E3 unmet, E1 and E2 partial, and E4 partial. C also leaves (ii) and (iii) unmet. |
| D-2 | The interim merge before (c) | A: the v6 merge rule with C1 to C10, then the lock fix. B: no interim rule. C: the lock fix alone on dev's resolver. | Decided: A, implemented in the commits `b496df9d8` and `7ab849b56`. The user chose "implement what you can, cleanly". | A is thrown away on v6 at Phase 3: about 1,530 plus 2,600 lines and the models. B: until Phase 3, the global cost-cursor lock bug keeps rejecting every charged offer outside the main parent in v6 merges. C: groups collapse on shared mergeable channels (`conflict_set_merger.rs:906-918`). |
| D-3 | The execution point and the order rule in (c) | A: execute only the finalized prefix. B: execute optimistically on the fork-choice chain, with rollback. Order rule: main-chain epochs, or height then hash. | A first, with main-chain epochs (§7.5). | A: results wait for finality. B: rollback complexity. Height then hash over finality batches diverges between nodes. |
| D-4 | Where acceptance binds in (c) | A: at execution, as P1 states. A2: A, plus a filter that counts the unexecuted demand of the past cone (§7.4). B: derived in-flight holds at inclusion. | A2. No hold crosses a block. The user must confirm that holds inside one block are exempt from the no-escrow rule (§7.6). | A: jointly overcommitted envelopes can fill blocks and then fail at no cost (P1:2208-2212, §7.4). A2: concurrent blocks can still overcommit a purse. B: a reservation table that the user's no-escrow rule forbids (`funding-settlement-design-review.md:171-176`). |
| D-5 | "Simultaneous arrival" in a DAG | A: only inside one block (DR-65). B: concurrent sibling blocks too. | A | A: alternatives in concurrent blocks resolve by the canonical order. B rejects many honest pairs. |
| D-6 | User alternatives | A: funding, slots and branches inside a deployment. B: a new envelope-level alternative group. | A. "D6 deferred" was an agent's default, not the user's decision. D6 was the label of this decision in the v6 merge-rule design report. | A: a user cannot rank the alternatives that the same user submits separately. B: not in the papers, size M, and a new consensus encoding. |
| D-7 | Two fully funded same-signer siblings under (a). §4.4 gives the full analysis. | A: keep one, the current default. B: redesign the cursor so that both survive. | A while (a) is an interim. Under (c), both execute in order. | A departs from P1:360-363. B is size M, and Phase 3 makes it unnecessary. |
| D-8 | The validator-set source under (c) | A: epoch-attested state (P1:3036-3043). B: a validator registry in consensus, outside RSpace. | A, with the lag of Q-3 | A: a new bond waits up to the lag. B: a new protocol element. P1 says that slashing, minting and stake-weighted voting are expressible as Rholang contracts (P1:3777-3780). |
| D-9 | Ownership | A: a new epic, owned with the Casper team. B: extend epic 8946. C: the Casper team alone. | A | B: the fixed exclusions of epic 8946 leave general consensus, finality and merge repairs to the Casper team. C: the cost-accounting invariants lose their owner. |

---

## 12. References

### 12.1 Governing papers

These papers are F1R3FLY publications without a digital object identifier (DOI). The links point to the [publications repository](https://github.com/F1R3FLY-io/publications) at revision `0bf7817`, the revision of every line reference.

- **[P1]** L. G. Meredith. *Cost-Accounted Rho Calculus: A Spectral Decomposition of Phlogiston.* F1R3FLY.io, May 2026. [`cost-accounting/cost-accounted-rho.tex`](https://github.com/F1R3FLY-io/publications/blob/0bf7817/cost-accounting/cost-accounted-rho.tex).
- **[P2]** L. G. Meredith. *Continued Interactive GSLTs and the Cost Endofunctor: A construction one level up from cost-accounted Rholang* (revised: wrapping by construction). F1R3FLY.io, May 2026. [`cost-accounting-as-monad/continued-gslt-cost-v2.tex`](https://github.com/F1R3FLY-io/publications/blob/0bf7817/cost-accounting-as-monad/continued-gslt-cost-v2.tex).
- **[ST]** L. G. Meredith. *Spacetime from Cost: A functor from cost-accounted ciGSLTs to measured causal sets.* F1R3FLY.io, June 2026. [`cost-spacetime/spacetime-functor.tex`](https://github.com/F1R3FLY-io/publications/blob/0bf7817/cost-spacetime/spacetime-functor.tex).

Two further papers of the same repository and revision are not governing papers. §3.7 cites them on scheduling. The amendment of 2026-10-13 added them.

- **[P3]** L. G. Meredith. *Knotted Topoi: the lift of the knotted set-theoretic universe, and fully abstract denotational semantics for the category of graph-structured lambda theories.* F1R3FLY.io, June 2026. [`knotted-topoi/knotted-topoi.tex`](https://github.com/F1R3FLY-io/publications/blob/0bf7817/knotted-topoi/knotted-topoi.tex).
- **[CS]** L. G. Meredith. *From the Axiom of Choice to Fair Scheduling: Toward a Curry–Howard Correspondence for Nondeterminism, in which choice principles are the types and fair schedulers are the terms.* Research note, F1R3FLY.io, June 2026. [`choice-types/choice-scheduling.tex`](https://github.com/F1R3FLY-io/publications/blob/0bf7817/choice-types/choice-scheduling.tex).

### 12.2 External works

Each DOI below was checked against Crossref and resolves at doi.org to the cited work.

- **[Androulaki et al. 2018]** E. Androulaki, A. Barger, V. Bortnikov, C. Cachin, K. Christidis, A. De Caro, D. Enyeart, C. Ferris, G. Laventman, Y. Manevich, S. Muralidharan, C. Murthy, B. Nguyen, M. Sethi, G. Singh, K. Smith, A. Sorniotti, C. Stathakopoulou, M. Vukolić, S. Weed Cocco, J. Yellick. "Hyperledger Fabric: A Distributed Operating System for Permissioned Blockchains." *Proceedings of the Thirteenth EuroSys Conference*, 2018, pp. 1-15. DOI: [10.1145/3190508.3190538](https://doi.org/10.1145/3190508.3190538).
- **[Bernstein 1966]** A. J. Bernstein. "Analysis of Programs for Parallel Processing." *IEEE Transactions on Electronic Computers* EC-15(5), 1966, pp. 757-763. DOI: [10.1109/PGEC.1966.264565](https://doi.org/10.1109/PGEC.1966.264565).
- **[Gelashvili et al. 2023]** R. Gelashvili, A. Spiegelman, Z. Xiang, G. Danezis, Z. Li, D. Malkhi, Y. Xia, R. Zhou. "Block-STM: Scaling Blockchain Execution by Turning Ordering Curse to a Performance Blessing." *Proceedings of the 28th ACM SIGPLAN Annual Symposium on Principles and Practice of Parallel Programming (PPoPP)*, 2023, pp. 232-244. DOI: [10.1145/3572848.3577524](https://doi.org/10.1145/3572848.3577524).
- **[Girard 1987]** J.-Y. Girard. "Linear Logic." *Theoretical Computer Science* 50(1), 1987, pp. 1-101. DOI: [10.1016/0304-3975(87)90045-4](<https://doi.org/10.1016/0304-3975(87)90045-4>).
- **[Kahn 1962]** A. B. Kahn. "Topological Sorting of Large Networks." *Communications of the ACM* 5(11), 1962, pp. 558-562. DOI: [10.1145/368996.369025](https://doi.org/10.1145/368996.369025).
- **[Kapritsos et al. 2012]** M. Kapritsos, Y. Wang, V. Quéma, A. Clement, L. Alvisi, M. Dahlin. "All about Eve: Execute-Verify Replication for Multi-Core Servers." *Proceedings of the 10th USENIX Symposium on Operating Systems Design and Implementation (OSDI 12)*, 2012, pp. 237-250. ISBN 978-1-931971-96-6. USENIX proceedings carry no DOI. The [USENIX page](https://www.usenix.org/conference/osdi12/technical-sessions/presentation/kapritsos) lists the paper. Added on 2026-10-13.
- **[LeBlanc and Mellor-Crummey 1987]** T. J. LeBlanc, J. M. Mellor-Crummey. "Debugging Parallel Programs with Instant Replay." *IEEE Transactions on Computers* C-36(4), 1987, pp. 471-482. DOI: [10.1109/TC.1987.1676929](https://doi.org/10.1109/TC.1987.1676929). Added on 2026-10-13.
- **[Meredith and Radestock 2005]** L. G. Meredith, M. Radestock. "A Reflective Higher-order Calculus." *Electronic Notes in Theoretical Computer Science* 141(5), 2005, pp. 49-67. DOI: [10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016).
- **[Schneider 1990]** F. B. Schneider. "Implementing Fault-Tolerant Services Using the State Machine Approach: A Tutorial." *ACM Computing Surveys* 22(4), 1990, pp. 299-319. DOI: [10.1145/98163.98167](https://doi.org/10.1145/98163.98167).
- **[Sompolinsky et al. 2021]** Y. Sompolinsky, S. Wyborski, A. Zohar. "PHANTOM GHOSTDAG: A Scalable Generalization of Nakamoto Consensus." *Proceedings of the 3rd ACM Conference on Advances in Financial Technologies (AFT)*, 2021, pp. 57-70. DOI: [10.1145/3479722.3480990](https://doi.org/10.1145/3479722.3480990).
- **[Sompolinsky and Zohar 2015]** Y. Sompolinsky, A. Zohar. "Secure High-Rate Transaction Processing in Bitcoin." *Financial Cryptography and Data Security (FC 2015)*, Lecture Notes in Computer Science, 2015, pp. 507-527. DOI: [10.1007/978-3-662-47854-7_32](https://doi.org/10.1007/978-3-662-47854-7_32). This is the GHOST rule that the Casper fork choice adapts.
- **[Thomson et al. 2012]** A. Thomson, T. Diamond, S.-C. Weng, K. Ren, P. Shao, D. J. Abadi. "Calvin: Fast Distributed Transactions for Partitioned Database Systems." *Proceedings of the 2012 ACM SIGMOD International Conference on Management of Data*, 2012, pp. 1-12. DOI: [10.1145/2213836.2213838](https://doi.org/10.1145/2213836.2213838).

The linear-logic reading of acceptance in P1:2318-2326 rests on [Girard 1987]. The rho calculus of P1 is the calculus of [Meredith and Radestock 2005].

### 12.3 Repository documents

- [Cost-accounting decision records](cost-accounting-decision-records.md). This document cites these records:
  - DR-11: a static linear-proof gate at block assembly
  - DR-15: run-to-completion and the retained merge dispatcher
  - DR-31: state-bound dependent admission closes ambient-cost undercount and bounds every user evaluation
  - DR-32: the atomic COMM is the only execution-cost event
  - DR-38: reserve and settle as one native step
  - DR-44: honest parent selection preserves the committed LFB state
  - DR-57: failed-body settlement remains an exact state effect
  - DR-64: the funding check before execution, with the unprovable remainder held to the signed limit
  - DR-65: same-block offers that share a purse form one funding decision
  - DR-66: the activation of the protocol-6 test genesis, and the retirement of the legacy meter
  - DR-67: stack safety for the recursion that cost accounting adds
  - DR-68: an installer-signed continuation draws on the installer's purse
  - DR-72: a failing offered candidate is quarantined, and the proposer tries the next one
  - DR-74: native evidence encodes each causal path as a delta from the previous path
  - DR-75: RSpace candidates are ordered by source hash, and only ties are digested
  - DR-78: produce-counter lookups charge the B-tree search bound
  - DR-79: replay authority tree visits charge the B-tree bound of the live map size
  - DR-84: a split adds one causal-path segment
  - DR-85: causal paths carry a chained digest, and the recording index compares digests
  - DR-86: native evidence interns its causal paths in one hash-consed trie
  - DR-96: a digest-keyed ordered index for the native session store
  - DR-100: replay charges that depend on neither the schedule nor a random shard
  - DR-101: system residue is never charged to an earlier deployment
  - DR-102: one replay charge for every role, and acceptance work on its own budget
  - DR-113: exhaustion of the signed limit is a classified user failure
  - DR-114: native funded execution records and replays external-service replies
  - DR-115: a failed offered deploy keeps its committed settlement in the merge index
  - DR-116: recovery carries offered envelopes
  - DR-117: each COMM authority region is validated once
  - DR-118: an introduction reuses the reducer's measurement of its value
  - DR-119: one creation lock for each bucket replaces the global cost-cursor lock
  - DR-120: the interim v6 merge rule, a fast path or one ordered pass
  - DR-121: names that a receive body creates are dynamic for the funding analysis
  - DR-122: the linear pre-execution cost estimate
  - DR-123: the check before execution of an offered deploy
  - DR-124: the live check holds only the client's demand
  - DR-125: a call of an unknown method is a classified user failure
- [Conformance catalog](cost-accounting-conformance-properties.md): the rows CA-P-171 and CA-P-172 (§5).
- [Merge-algebra dossier](merge-algebra/merge-algebra-specification.md): the determinism contract of dev's merger.
- [Finalized-floor dossier](finalized-floor/finalized-floor-specification.md): the floor rule and the LFB derivation.
- [Fork-choice dossier](fork-choice/fork-choice-specification.md): LMD-GHOST and its proofs.
- [Slashing design, chapter 07](slashing/design/07-fork-choice-and-lifecycle.md): the fork-choice exclusion theorem T-10.
- [Funding-settlement design review](cost-accounting-impl/funding-settlement-design-review.md): the no-escrow rule.
- [Ratification status ledger](../design/cost-accounting-ratification-status.md).
- [Settled-effect probe claim](../../claims/settled-effect-probe-equivalence.md): the August soak attribution.
- [Casper soak merge-accounting claim](../../claims/casper-soak-merge-accounting.md).
- [Task board](../../ToDos.md): EPIC-021 and the October soak data.
- [Casper theory index](README.md).
- [Deterministic parallel reduction](cost-accounting-impl/deterministic-parallel-reduction.md): the scheduler document of §3.7. Added on 2026-10-13.
- Dev's `docs/rholang-evaluation-order.md` at dev `a06e3e7b3`: the evaluation model of dev, which §3.7.1 cites. This branch carries its own edit of the same file. Added on 2026-10-13.
- Two local artifacts that are not in the repository: the design study `target/scratch/scheduler-removal-design-v1.md` (§3.7) and the restoration plan `target/scratch/m34-restoration-plan.md` (§3.8). Added on 2026-10-13.
