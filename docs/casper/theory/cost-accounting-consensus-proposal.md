# Cost Accounting and Casper: Remaining Gaps and an Order-Then-Execute Proposal for Cost-Accounted (v6) Shards

| Field | Value |
|---|---|
| Status | Proposal for discussion with the Casper team. Not ratified. |
| Date | 2026-10-09 |
| Audience | The Casper team of F1R3FLY, the user (the developer who directs this work), and Greg (L. G. Meredith), the author of the cost-accounting papers |
| Code basis | Branch `feature/cost-accounted-rho` at `17e07307f`. It contains `dev`, the integration branch of the F1R3node repository, at `93e38575e`. |
| Paper basis | The F1R3FLY publications repository at revision `0bf7817` |
| Authority | The [ratification status ledger](../design/cost-accounting-ratification-status.md) applies. An implementation record does not ratify a Casper change. |

## Contents

1. [Purpose and summary](#1-purpose-and-summary)
2. [Background for Casper readers](#2-background-for-casper-readers)
3. [Today's pipeline](#3-todays-pipeline)
4. [What is being implemented now](#4-what-is-being-implemented-now)
5. [Prior implementation on feature/casper-cost-accounting-completion](#5-prior-implementation-on-featurecasper-cost-accounting-completion)
6. [Remaining gaps](#6-remaining-gaps)
7. [The recommended architecture: order-then-execute for v6 shards](#7-the-recommended-architecture-order-then-execute-for-v6-shards)
8. [Change analysis](#8-change-analysis)
9. [Performance analysis](#9-performance-analysis)
10. [Phased plan](#10-phased-plan)
11. [Open decisions](#11-open-decisions)
12. [References](#12-references)

## Reading guide

**Evidence tags.** Each claim carries one tag or more.

| Tag | Meaning |
|---|---|
| [C] | Code, with a `file:line` reference. The code is at HEAD unless the text names another commit. HEAD is the commit `17e07307f` of the code basis. |
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
| HEAD, commit IDs | HEAD is the commit `17e07307f`. A hexadecimal string such as `1d325b996` is the ID of a git commit. The word "session" marks the ID of a codex session instead (§5). | This guide |
| E1 to E4 | Greg's four expectations: E1 merge logic, E2 user alternatives, E3 keep accepting blocks, E4 performance | §1.2 |
| (a), (b), (c), (c)+(b), (d1), (d5) | The candidate designs. "(a)+(b)" in §11 combines (a) and (b). | §1.4 |
| Epic 8946, bug 11004 | Two items in pgmcp, the work tracker of this effort: the epic "Complete offered-funded production cost-accounting integration", and the global cost-cursor lock bug | §2.1, §2.4 |
| G1, G2, G3, G6, G9 | Gap labels of epic 8946. G1: the funding check before execution. G2: several offers per block. G3: installer funding. G6: the recovery of offered envelopes. G9: the v6 merge rule. | §2.1 |
| DR-*n* | Decision record *n*. DR-119 and DR-120 are the records of the cost-cursor lock fix and of the interim v6 merge rule. | §12.3 |
| Class I, class S, class D | The three conflict classes under cost accounting: independent, same purse, and shared data | §2.4 |
| F1 to F8 | The eight fast-path conditions of the v6 merge rule | §4.4 |
| C1 to C10 | The ten changes that an independent arbiter required of an earlier version of the v6 merge-rule design | §4.4 |
| HIGH-*n*, MEDIUM-*n*, LOW-*n* | The findings of the third independent check of that design, with their levels | §4.4 |
| CA-P-*n* | Row *n* of the cost-accounting conformance catalog | §5 |
| GAP-E1 to GAP-E4, GAP-i to GAP-iv | The remaining gap against one expectation, or against one item of the new model of P1. The items are (i) explicit transaction boundaries, (ii) concurrent acceptance, (iii) throughput, and (iv) merge via signatures and cost accounting. | §6.0 |
| A-LIN | The assumption that each LFB lies on the main chain of every later LFB | §7.5 |
| Q-1 to Q-6 | The technical questions for the Casper team | §7.13 |
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

"P1:344-366" means lines 344 to 366 of P1.

**Code.** A reference such as `runtime.rs:359` uses the short file name of this key. Paths are relative to the repository root.

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
| `production_limits.rs` | `casper/src/rust/util/rholang/costacc/production_limits.rs` |
| `acceptance.rs` | `casper/src/rust/util/rholang/acceptance.rs`, only at the commit `f9bd3895d` (§5) |
| `SystemVault.rho` | `casper/src/main/resources/SystemVault.rho` |
| `MakeMint.rho` | `casper/src/main/resources/MakeMint.rho` |
| `offered_funded_api_test.rs` | `casper/tests/api/offered_funded_api_test.rs` |
| `offered.rs` | `models/src/rust/signed_phlo_deploy/offered.rs` |
| `block_metadata.rs` | `models/src/rust/block_metadata.rs` |
| `cost_protocol_limits.rs` | `models/src/rust/cost_protocol_limits.rs` |
| `CasperMessage.proto` | `models/src/main/protobuf/CasperMessage.proto` |
| `block_dag_key_value_storage.rs` | `block-storage/src/rust/dag/block_dag_key_value_storage.rs` |
| `deterministic_reduction.rs` | `rholang/src/rust/interpreter/deterministic_reduction.rs` |
| `rholang_merging_logic.rs` | `rholang/src/rust/interpreter/merging/rholang_merging_logic.rs` |
| `DeterministicParallelReduction.v` | `formal/rocq/cost_accounted_rho/theories/DeterministicParallelReduction.v` |

"DR-*n*" names record *n* of the [decision records](cost-accounting-decision-records.md). Each decision record gets a short description at its first use, and §12.3 lists them all. Two of them are new. DR-119 records the cost-cursor lock fix (§4.5). DR-120 records the interim merge rule for cost-accounted (v6) shards (§4.4).

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

Greg is the author of the cost-accounting papers. The user relayed his four expectations verbatim.

| ID | Short name | Expectation (verbatim) |
|---|---|---|
| E1 | Merge logic | "The new cost-accounting changes eliminate the majority if not all the merge logic." |
| E2 | User alternatives | "users may decide which alternatives they want which eliminates merging." |
| E3 | Keep accepting blocks | "You no longer need to run to completion -- you can just keep running and keep accepting blocks." |
| E4 | Performance | "the majority of the performance issues in F1r3node are caused by the existing merge logic and that the cost-accounting changes should eliminate most of them." |

### 1.3 What the papers say

P1 starts from two defects of the current node. [P]

- **Run-to-completion.** RSpace, the tuple-space store of the node, "accepts a deployment, executes it to termination (or until phlogiston is exhausted), commits the resulting state changes, and only then accepts the next deployment" (P1:259-262).
- **Merge analysis.** "Merge analysis is expensive, complex, and a persistent source of bugs." (P1:300-301). "Wasted computation is the norm, not the exception." (P1:306-307).

P1 replaces run-to-completion with "concurrent acceptance gated by linear resource proofs" (P1:313-314). Conflicts "are detected *at acceptance time* through the linear resource proof" (P1:346-348). [P]

**Scope of the papers.** P1 is explicit for one RSpace and one validator. It states its arrival rules for deployments that "arrive at a validator" (P1:2247). P1 does not say how a multi-validator block DAG (the directed acyclic graph of blocks) orders acceptances. Every mapping of P1 onto Casper in this document is therefore an interpretation. [P][I]

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
2. **Order-then-execute on cost-accounted (v6) shards meets all four expectations.** [I] E4 still needs confirmation by measurement. Consensus orders the accepted envelopes, which are signed user deploys. A deterministic executor runs the finalized order and applies the acceptance gate of P1 (§2.2) before each envelope (§7).
3. **Order-then-execute remains Casper.** [I] It keeps the justifications of Casper CBC (correct by construction), the fork choice, the clique-oracle finality and the equivocation slashing. It changes when state is computed and where the validator weights come from (§7.8, §7.9).
4. **E4 is not yet proven.** [M][I] The data of the August soak test (a long load test of a validator network) agree with E4. Dev's later fixes moved the visible costs to the per-block cycle of state reset, replay and checkpoint (§2.1, §9).
5. **The earlier branch does not change this assessment.** [C][I] A review of the earlier branch `feature/casper-cost-accounting-completion` found that its tip met none of E1 to E4 (§5). It met E2 in part, but only inside one proposer's block and only under a narrow reading in which funding decides. No gap of §6 closes, and §7 to §11 do not change.

Two further gaps affect every design (§6.10). The first is the committed host-work caps, which are limits on deterministic node work per deploy. Today they reject the testnet log initializer of Embers, the F1R3FLY blockchain application programming interface (API). The second is unmetered continuations on the settlement log channels.

| Design | E1 merge logic | E2 user alternatives | E3 keep accepting blocks | E4 performance |
|---|---|---|---|---|
| (a) | Partial | Partial | Unmet | Marginal |
| (b) | Unmet | Partial | Partial | Partial |
| (c) | Met on v6 | Met | Met | Largest gain, not yet measured |
| (c)+(b) | Met on v6 | Met | Met | Largest gain, not yet measured |
| (d1) | Partial | Met for funding | Unmet | Small |
| (d5) | Mostly | Unmet | Unmet | Mixed |

### 1.5 What this document asks of the Casper team

1. Review the gap analysis (§6) and the change analysis (§8).
2. Decide the open decisions D-0 to D-9 together with the user and Greg (§11).
3. Answer the technical questions Q-1 to Q-6 (§7.13).
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
| Demand $`\Delta_s`$ | The number of $`s`$-signed layers that a deployment can force, over its whole call graph. | P1:2002-2027 |
| Linear resource proof | The inequality $`\Sigma_s \geq \Delta_s`$ for every signature $`s`$ of a deployment. Each token is used exactly once. Here *linear* refers to linear logic. | P1:2047-2058, 2318-2326 |
| Acceptance, the gate | The validator computes the demand before any execution, compares it with the supply, and then accepts or rejects. | P1:2188-2213 |
| Hold | The part of a purse that acceptance commits to one deployment until settlement. Other deployments cannot use it. | P1:2301-2302, DR-64 (the funding check before execution) |
| Settlement | The step after execution that charges the measured use, transfers the fee and refunds the unused hold. | P1:2132-2136, DR-38 (reserve and settle as one native step), DR-64 |
| Cost cursor | Revision and position cells in SystemVault for one charge scope. Each charged settlement advances them. Today every cursor check takes one global lock. | `SystemVault.rho:52-56, 145-185` |
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
| IntegerAdd channel | A channel that holds one number. A merge combines concurrent changes to it by addition. | `rholang_merging_logic.rs:237-267` |
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
| Folded mixing | One branch changes a mergeable channel by a folded diff, and another branch writes the same channel as a plain value. Dev's merger rejects the mix. | `dag_merger.rs:437-461` |
| Over-fill | A merge result in which a single-value cell would hold more than one datum | [Merge-algebra glossary](merge-algebra/merge-algebra-glossary.md) |
| Rejected records | The deploys that a merge rejects. A block records them, and every validator recomputes them. | `CasperMessage.proto:554-575` |
| Option search | The adjudication of dev's merger. It lists the sets of branches that it could reject, and it picks one by prior losses and then by cost. | `conflict_set_merger.rs:864-974` |
| Pinned chain, settled chain | A chain whose effects are already in the state that the merge builds on. A merge never rejects it. | `conflict_set_merger.rs:134-144`, `dag_merger.rs:1854-1915` |
| Late chain | A chain whose merge window has closed. The merge rejects it and every chain that depends on it. | `dag_merger.rs:944-948` |
| Prior losses | The number of earlier merges that rejected a chain. A chain with more losses gets priority. | `conflict_set_merger.rs:1012-1018` |
| Lineage closure | After a merge rejects chains, it also rejects each unpinned chain whose block descends from a block with a rejected chain. | `dag_merger.rs:1854-1915` |
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

Today the merger discovers conflicts after execution, from the event logs of the executed deploys. P1 replaces that analysis with three classes that the signatures already determine (P1:356-366). [P]

> "deployments signed by *different* signatures draw from *disjoint* token pools and cannot conflict. They may be executed in parallel and their results composed without merge analysis. Deployments signed by the *same* signature (or overlapping compound signatures) compete for the same token pool and are serialized by the linear proof --- again without merge analysis. The only case requiring attention is deployments that interact via shared data channels, and even here the cost-accounting structure provides a natural serialization order." (P1:356-366)

![Activity diagram of the three conflict classes under cost accounting. Two deployments D1 and D2 enter. The first question asks if they draw on a common purse. A common purse means the same signature or overlapping compound signatures. If yes, they are class S, and the linear proof serializes them. In sequence, the first is accepted and the second is rejected if the remaining supply is short (P1:2245-2269). Together, they form one parallel composition, and both are rejected if the supply is short (P1:2271-2287). An indigo note says that P1 states both rules for one validator only. If they share no purse but share a data channel, they are class D, which an order serializes (P1:363-366). Otherwise they are class I. They run in parallel and compose without merge analysis (P1:356-359). A red note says that today every class goes through post-hoc merge analysis. Another red note says that the global costCursorLock makes every pair of charged offered siblings a false class S conflict. That note names the global cost-cursor lock bug, pgmcp bug 11004. A legend explains the colours. It also defines P1, the deployments D1 and D2, their demands, and the supply of the common purse. It defines the classes S, D and I, HEAD 17e07307f, pgmcp bug 11004 and DAG.](diagrams/consensus-proposal-conflict-classes.svg)

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

| Class | What decides it under cost accounting | What decides it today |
|---|---|---|
| I, independent | Nothing. The results compose. | The merger, after execution |
| S, same purse | The gate, in arrival order | The merger, after execution |
| D, shared data | An order. Under §7 it is the canonical order. | The merger, after execution |

The global cost-cursor lock bug (pgmcp bug 11004) adds a fourth effect today. The global lock `costCursorLock` (`SystemVault.rho:52-56`) is consumed by every cursor check (`SystemVault.rho:145-167`). It therefore puts every pair of charged offered siblings into a false class S conflict. DR-115, the record that a failed offered deploy keeps its settlement in the merge index, states this under "Merge outcomes". [C]

### 2.5 What the papers do not specify

The following points are outside the papers. This document marks every rule for them as an inference. [P][I]

- One acceptance order across several validators. P1 states arrival rules for one validator (P1:2247, 2272-2273).
- The lag between consensus and execution. P1 speaks of RSpace that accepts deployments "at any time" (P1:327-328), not of blocks.
- "Keep accepting blocks" (E3) extends P1 from RSpace to blocks. The extension is Greg's.
- An envelope-level group of alternatives. No paper defines one.
- A text search of six companion papers found no rule for block ordering or block-state merging. They are the papers on choice types, consensus types, rent, history and cost, virtual tokens and radical fault tolerance.

---

## 3. Today's pipeline

This section describes the code at HEAD on a v6 shard. Legacy shards run the same Casper path with legacy deploys.

![Sequence diagram of today's execute-then-merge pipeline on a v6 shard at HEAD 17e07307f. The participants are the signing user, the proposer, the merge, the play runtime, the validator, the replay and the finalizer. In the proposal phase, the signing user sends a signed offered envelope. The proposer selects at most one offer. It merges the parent states first, and the merge chooses survivors after execution. A red note says that the proposer cannot run a deploy before this merge. The play runtime then executes the single envelope, funds and settles it, replays it and runs the system deploys. A red note says that a v6 block holds one envelope and that the legacy loop plays each deploy to termination. In the validation phase, every validator recomputes the merge. It compares the pre-state, the rejected records and the state facts. It waits for the one-permit ReplayLock and replays the block to termination. A red note says that acceptance waits for execution. The validator builds the eager merge index and adds the block to the DAG. In the finality phase, the finalizer derives the floor of the live view. It reads the weight map from the main parent's bonds, which execution wrote into the block. A legend explains the colours. It also defines HEAD 17e07307f, v6 shard, offered envelope, CloseBlock, ReplayLock, DAG, LFB, floor, and the file and line notation.](diagrams/consensus-proposal-today-pipeline.svg)

*Source: [`diagrams/consensus-proposal-today-pipeline.puml`](diagrams/consensus-proposal-today-pipeline.puml). Render it with `./render.sh consensus-proposal-today-pipeline.puml` in `docs/casper/theory/diagrams/`.*

### 3.1 Proposal

1. The proposer selects at most one offered envelope. Every further envelope becomes an alternate (`block_creator.rs:1847-1916`, the rule at `:1910-1916`). [C]
2. `compute_deploys_checkpoint_envelopes` logs "merge parents, then run deploys" (`interpreter_util.rs:912`). It merges first (`interpreter_util.rs:930-940`) and plays afterwards (`interpreter_util.rs:995-1005`). [C]

### 3.2 Merge

1. `compute_parents_post_state` (`interpreter_util.rs:1212`) dispatches on the number of parents (`interpreter_util.rs:1244-1319`). A single parent gives its post-state only when its lineage holds the derived floor. Otherwise the block re-bases through the full merge (`interpreter_util.rs:1311-1319`). [C]
2. With several parents, the full merge is `dag_merger::merge` (`interpreter_util.rs:1994`, `dag_merger.rs:820`). [C]
3. The merger chooses the rejected branches by an option search after execution (`conflict_set_merger.rs:864-974`). It groups every pair of branches that share any mergeable channel (`conflict_set_merger.rs:900-912`). With pinned branches it searches exhaustively (`conflict_set_merger.rs:873-883`). [C]
4. The merge results are consensus content: the pre-state root, the rejected records, the applied-from-scope set and the merge base (`CasperMessage.proto:554-575`). [C]

### 3.3 Play and run-to-completion

1. `compute_state_envelopes` (`runtime.rs:340-669`) accepts exactly one envelope (`runtime.rs:359`). A second envelope is an error (`runtime.rs:641-650`). [C]
2. The offered path executes first (`runtime.rs:480-493`). It funds and settles the candidate afterwards (`runtime.rs:519-539`). DR-64 records the same fact: the static analysis existed, "but no production path called them". [C]
3. The producer replays its own candidate before it publishes (`runtime.rs:574-582`). It then runs the system deploys (`runtime.rs:620-629`). [C]
4. The legacy loop plays each deploy to termination before the next one (`runtime.rs:732-753`). A play budget can defer the remaining deploys to a later proposal (`runtime.rs:732-734`). [C]

### 3.4 Validation and replay

1. `run_validation_steps` (`validation_dispatcher.rs:84-334`) checks the block summary (`:99-118`) and the offered-only rule (`:124-130`). It then runs the checkpoint step. [C]
2. The checkpoint step recomputes the merge from the block's own justifications (`interpreter_util.rs:418-433`). It compares the pre-state (`:461-470`), the rejected records (`:471-518`) and the state facts (`:519-532`). Then it replays the block (`:538-545`). [C]
3. A node replays one block at a time. The `ReplayLock` is a semaphore with one permit (`runtime_manager.rs:211-218`). The wait for it has its own metric (`runtime_manager.rs:1264-1267`). [C]
4. Offered replay requires "one isolated user candidate" (`runtime_manager.rs:1737-1741`). [C]
5. After validation, every block gets a merge index when the shard allows more than one parent (`validation_dispatcher.rs:340-375`). The guard is at `:345`. [C]

### 3.5 Finality

1. Every `finalization_rate` blocks, the node starts the finalizer (`finalization_runner.rs:662-701`). [C]
2. The finalizer derives the floor of the live view (`finalization_runner.rs:493-560`, `floor.rs:517-643`). It adopts a new LFB only when the derived state contains the settled effects of the current LFB (`floor.rs:615-638`). [C]
3. The clique oracle reads the weight map of the target's main parent (`clique_oracle.rs:202-220`). That map comes from the block's `bonds` (`block_metadata.rs:141-162`), which execution writes into the post-state fields (`CasperMessage.proto:597-605`). [C]

### 3.6 Where run-to-completion and the merge happen

| Place | What waits, or what is repeated | Code |
|---|---|---|
| Block play | Each deploy runs to termination before the next | `runtime.rs:732-753`, `:359` |
| Proposal | Play waits for the merge of the parents | `interpreter_util.rs:930-940` |
| Validation | Acceptance waits for the merge recomputation and the replay | `interpreter_util.rs:400-555` |
| Replay | One replay at a time on each node | `runtime_manager.rs:211-218` |
| Producer | The producer replays its own candidate | `runtime.rs:574-582` |
| Native frontier | One conflict component for each native frontier inside one deploy | `deterministic_reduction.rs:1068-1072, 1120-1122` |
| After validation | A merge index for every block | `validation_dispatcher.rs:340-375` |

**Note on DR-15.** DR-15 item 1 states that run-to-completion "was never ported" and that the acceptance gate of DR-11 is live. DR-11 is the record of a static linear-proof gate at block assembly. At HEAD, the code above plays and replays each deploy to termination. No file under `casper/src` or `node/src` calls `admit_by_funding`, `delta_sigma` or `static_authority_plan`. [C] The merge `1d325b996` restored dev's Casper code on 2026-10-01 (§5). The v6 merge-rule work plans a correction of DR-15. [C]

---

## 4. What is being implemented now

**Status on 2026-10-10: designed, checked and approved.** The user approved the designs of all five items on 2026-10-10. The preparatory steps of G1 are committed. The v6 merge rule and the cost-cursor lock fix are committed. The full-cost analysis of G1 comes next. G2 and G3 are not started. The orchestrator updates this section.

The user directed the lane to "Implement what you can, cleanly, without a lot of changes to casper". The five items below follow that rule. Each one changes Casper only at a v6 seam or in cost-accounting code.

Two ownership boundaries apply to this work.

1. **Casper.** The lane changes Casper only where cost accounting requires it. General Casper findings go to the Casper team as reports. The lane does not fix them.
2. **Stack safety.** Another team owns stack safety for the interpreter: the MeTTaIL and F1R3Lang language effort, on the branches `feature/mettail`, `feature/f1r3lang-mettail-only` and `integration/f1r3lang-cost-accounted-rho-20261005`. The lane backports that team's existing code where cost accounting needs it, with a citation. It writes new stack-safe code only for cost-accounting modules that no branch covers.

### 4.1 G1: the funding check before execution (DR-64)

**Status: approved on 2026-10-10 after two independent checks. The preparatory steps G1-1 to G1-3 are committed. The full-cost analysis comes next.** The design document is [the linear pre-execution cost estimate](cost-accounting-impl/linear-cost-estimate.md).

- **Problem.** The offered path executes the candidate and checks the funding afterwards (`runtime.rs:480-539`). [C] P1 requires the check "**Before** executing any part of the deployment" (P1:2191). [P]
- **Change.** Before execution, the producer and every validator compute the known demand $`\Delta^{\mathrm{known}}_s`$ of each signature lane $`s`$ from the envelope and $`R_0`$. A signature lane is the part of the demand that one signature pays. The known demand counts communications plus introduction, transfer and trace bytes. One analysis computes it as the demand side of the linear resource proof. The same pass decides whether the estimate is complete, which means that no dynamic logic can add cost. A complete estimate is held exactly, plus the fee, and no remainder is held. For an incomplete estimate, a construct that the analysis cannot prove contributes 0, and the unprovable remainder $`R`$ is held in full. Here $`\mathrm{phloLimit}`$ and $`\mathrm{phloPrice}`$ are the signed phlo limit and the signed phlo price of the envelope:

```math
R \;=\; \Bigl(\mathrm{phloLimit} - \sum_{s} \Delta^{\mathrm{known}}_s\Bigr) \cdot \mathrm{phloPrice}.
```

- **Rejection.** If the holds do not fit the supply at $`R_0`$, the candidate never enters the runtime. The rejection has no effect and no charge (DR-64 decision 5).
- **Casper footprint.** None for the check itself. The user moved the code motion of G2 ahead of the live check. That step moves the single-offer play and replay code out of `runtime.rs` and `runtime_manager.rs` into cost-accounting code. Then the check lives entirely in cost-accounting code: the analysis, a new gate module and the offered execution path (`execution.rs:272-443`). Settlement keeps charging the measured use directly, so no hold floor is added. No message format changes. Legacy shards are unchanged.
- **Analyzers.** The check uses the static demand analyzer (`delta_sigma.rs`) and the lexical name resolver (`lexical.rs`). Both recurse today, so a deeply nested deploy could overflow a node's stack. The F1R3Lang branches already have iterative versions of the analyzer's four walks (merge commit `29b729551`). The lane backports them verbatim, with a citation. No branch makes the resolver metered, linear or stack-safe. The lane therefore adds those three properties itself, in the style of the stack-safe pushdown automata (PDAs) of `feature/mettail`.
- **A defect in the resolver.** The resolver gives a name created inside a receive body the parent's randomness (`lexical.rs:53-56`). The runtime builds that name from the received data (`dispatch.rs:63-79`). Such names must count as dynamic demand, so the held limit covers them. Step G1-3 fixed this defect, and DR-121 (names that a receive body creates are dynamic) records the fix. [C]
- **Effect on clients.** [I]
  - An offer with an incomplete estimate is accepted only if its eligible sources can hold $`\mathrm{phloLimit} \cdot \mathrm{phloPrice} + 1`$ at $`R_0`$. Most real deploys call stored contracts, so most estimates are incomplete. Some offers that succeed today will therefore be rejected.
  - The estimate counts each communication once for each participant. It can therefore reach twice the realized use in compute, transfer and trace. An offer whose estimate exceeds its signed limit is rejected.
  - Clients such as Embers must size the phlo limit to their balances and to the estimate. The estimate depends only on the envelope and on public consensus inputs, so a client can compute it before signing.
  - An offer with several sources needs one more rule, which [Section 14 of the design document](cost-accounting-impl/linear-cost-estimate.md#14-the-client-sizing-rule) states and proves.
- **Open points.** [C][I]
  - Before the check goes live, the offered test fixtures must fit their holds. An incomplete estimate holds the whole signed limit.
  - The shared helper (`casper/tests/helper/offered_deploy.rs:49-52`) signs a hold of 200,000,001 against test vaults of 9,000,000. About 52 tests that pass today would therefore be rejected at the check.
  - The user decided on 2026-10-10 to raise the funded test vaults to $`10^{12}`$.
  - The 64-source test needs no change. Its term is complete, so its holds land per class of charge on the one source of each class.
- **Verification.** `EndToEndAuthority.v` must extend its branch lemmas to the held remainder. Tests must show that an underfunded offer never enters the runtime.

### 4.2 G2: several offered deploys per block as an ordered batch executor (DR-65)

**Status: approved on 2026-10-10. Not started.**

- **Problem.** A block holds at most one offer (`block_creator.rs:1910-1916`). Replay requires one isolated candidate (`runtime_manager.rs:1737-1741`). [C]
- **Change.** A block holds several offers in canonical order. Member $`i`$ funds from $`R_0`$ and executes from $`R_{i-1}`$, the root after member $`i - 1`$. Members that share a purse form one group. Let $`\mathrm{hold}_m(p)`$ be the hold of member $`m`$ on purse $`p`$, and let $`\mathrm{supply}_{R_0}(p)`$ be the balance of $`p`$ at $`R_0`$. If one purse $`p`$ of a group $`G`$ has $`\sum_{m \in G} \mathrm{hold}_m(p) > \mathrm{supply}_{R_0}(p)`$, every member of $`G`$ is rejected. This is P1's simultaneous-arrival rule (P1:2271-2287). A held-capacity guard keeps the holds of later members available.
- **Design choice.** The member loop is built as an ordered batch-executor API. The parallel executor of §7.10 can then parallelize it, and the executor of §7.6 can feed it finalized blocks. [I]
- **Casper footprint.** A refactor first moves the single-offer play code out of `runtime.rs` into a new cost-accounting module. That step changes about −230 and +25 lines in `runtime.rs`. Then `runtime_manager.rs` (about 40 lines), `replay_runtime.rs` (about 10), `block_creator.rs` (about 45) and `errors.rs` (about 10) change. All changes sit behind the v6 seam.
- **Decided points.** [I]
  - Member $`i`$ is accepted at $`R_0`$ but reads its settlement inputs at $`R_{i-1}`$. Two offers of one signer share a cursor scope, so a read at $`R_0`$ makes the second settlement fail. This amends DR-65, and the user approved it on 2026-10-10.
  - The guard is item 5 of DR-65 as adopted. Here $`\mathrm{balance}_{\mathrm{after}}(p)`$ is the balance of purse $`p`$ after member $`i`$ executes, and $`\mathrm{debit}_i(p)`$ is the debit of member $`i`$ on $`p`$, fee included. The guard requires $`\mathrm{balance}_{\mathrm{after}}(p) - \mathrm{debit}_i(p) \geq \sum_{j>i} \mathrm{hold}_j(p)`$. Settlement charges the measured use directly, so this one condition is exact. An earlier draft added a second condition, which only a settlement floor needed.
  - The design also caps the funding capacity of each member on a purse at the purse balance minus the later holds. The cap applies on the success path and on the rollback path, so a member never draws on funds that a later member holds.
- **Verification.** The planned model `SameBlockFundingGroups.tla` must check GroupAllOrNone, NoOverdraft, GateBeforeExecute, HeldCapacityGuard and DecisionPermutationInvariant. Negative controls must cover prefix admission and a missing held guard. Rocq must prove the three group lemmas of DR-65.

### 4.3 G3: installer funding (DR-68)

**Status: approved on 2026-10-10, including the two changes to DR-68 that the decided points below describe. Not started.**

- **Problem.** Funding eligibility admits only the sources of the triggering envelope (`family_selection.rs:163-186`). [C] A continuation that an installer stored therefore cannot draw on the installer's purse, and the gateway flow fails (DR-68 context).
- **Change.** The installer's located purse becomes an eligible source for obligations in the installer's stored signed region. A signed region is the part of a term under one signature $`s`$, as in $`\{P\}_s`$. Its seal is the stored record of that signature. When that budget is exhausted, the interaction does not fire and has no charge. This is the token gating of P1's rules.
- **Decided points.** [C][I]
  - "Does not fire" needs a veto in the RSpace accounting observer, and no veto exists today. A denied COMM aborts the deploy (`rholang/src/rust/interpreter/accounting/mod.rs:2079-2084`). Until a veto exists, the node rejects the triggering deploy with no charge. This interim rule deviates from DR-68, and the user approved it on 2026-10-10. The veto stays a tracked follow-up.
  - `eval_cost_signed_term` (`rholang/src/rust/interpreter/reduce.rs:1920`) does not check the signer.
  - User syntax cannot create a key signature. Key regions come only from the deploy's own funding signature (`rholang/src/rust/interpreter/accounting/mod.rs:1359-1399`), and `vault_payer` maps only key-shaped signatures to a wallet (`casper/src/rust/util/rholang/costacc/vault_payer.rs:28-61`).
  - User syntax can still create spelled, quoted and held-name signatures. So G3 funds only seals whose signature locates a key wallet, and only seals that existed at $`R_0`$. This narrows DR-68, and the user approved it on 2026-10-10.
- **Casper footprint.** Cost-accounting code only. No dev Casper file changes.

### 4.4 The interim v6 merge rule (DR-120)

**Status: implemented and committed as `b496df9d8` on 2026-10-10.** Design v4 resolves every finding of the third independent check, and the user approved it on 2026-10-10.

**Purpose.** v6 shards merge until the architecture of §7 exists. The rule replaces dev's option search on v6 shards only. Legacy shards run dev's merger byte for byte. [I]

**Shape.** The rule comes from the deploy formats. A merge is a v6 merge when a parent or scope block other than genesis holds an offered user deploy. Validation already rejects mixed formats on v6 shards, so the rule needs no shard flag. The user chose this variant on 2026-10-10, because it removes three Casper files from the change. The rule has two paths below dev's dispatcher.

- **Fast path.** When the eight fast-path conditions F1 to F8 of the table below all hold, the rule composes all branches with `compute_merged_state` and rejects nothing.
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

The slow path is one pass in a strict total order $`K`$ on branches. The order puts settled (pinned) branches first. It then ranks branches with more prior losses first, then lower height, and then by dev's branch comparator (`conflict_set_merger.rs:40-78`).

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

A failure prefers a contributor whose diff has the failure's sign. If no such contributor exists, for example on a negative base, the pass drops a contributor of either sign. Each trim round therefore removes at least one branch, and the pass terminates. The composition folds the survivors in canonical order (`conflict_set_merger.rs:458-468`), so the result does not depend on hash-set order. [C][I]

**What it eliminates.** [C][M]

| Measure | Fast path | Slow path |
|---|---|---|
| Dev's Casper merge lines that do not run (of 4,477) | about 2,828 (63 %) | about 887 (20 %) |
| Net merge logic eliminated | about 56 % | not measured |
| CPU (central processing unit) time on the measured tests | adjudication, about 1 to 1.5 % of test work | the same |
| What stays | the eager merge index, 7.8 to 10.5 % of test work | the same |

The line counts come from the step inventory of the v6 merge-rule design. It counts non-blank, non-comment lines at HEAD. The 63 % counts dev lines that the fast path does not run. It is not the amount of logic removed. The arbiter of the next paragraph required this label as its change C8.

**Arbitration.** An independent arbiter checked version 2 of the design and accepted it with ten required changes, C1 to C10. Each change has a level. HIGH marks a defect that must close before implementation. MEDIUM marks a required change of the design, its proofs or its plan. LOW marks a precision fix. [I]

| ID | Level | Required change |
|---|---|---|
| C1 | HIGH | Add dev's folded-mixing rejection (`dag_merger.rs:437-461`) to F7 and to the pass. |
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

**Verification plan.**
- **Rocq.** `V6MergeOrder.v`, `V6MergeLedger.v` and `V6MergeOrderedPass.v` must prove order independence, conflict freedom, ledger validity, termination and fast-equals-slow. `CursorLinearity.v` and `CostCursorBuckets.v` cover the cursor arguments. Negative controls include an arrival-order tie-break and a missing claim check. Two more cover a set collapse that hides a duplicate and a sign-only repair that stalls on a negative base.
- **TLA+.** `V6MergeSerialization.tla` must check three validators with permuted arrival orders. Ten failing configurations must each refute one named invariant. `CursorDuplicateDetection.tla`, `FeeCursorBranchMerge.tla` and `FeeCursorCells.tla` cover the cursors and the lock fix.

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

**Status: implemented and committed as `7ab849b56` on 2026-10-10.** The only Casper file that it changes is `SystemVault.rho`, with 88 lines added and 18 removed. Each replaced line stays as a comment that states its reason.

- **Problem.** `SystemVault.rho:52` declares one lock, and `:56` produces it once. `ensureCostCursor` consumes it on every call (`SystemVault.rho:145-167`). Every charged settlement therefore writes one shared channel. [C] P1 requires that "RSpace is never locked by a single deployment" (P1:333-334). [P]
- **Effect.** Any two charged offered siblings conflict in a merge. A test relies on this fact today (`offered_funded_api_test.rs:1858-1861`). [C]
- **Change.** A settlement with an existing cursor takes no lock. A new cursor takes one of 256 bucket locks, keyed by the first byte of the Keccak-256 hash of its scope. The code checks again under the lock. Only cost-cursor code changes. The vault-creation code keeps dev's version, and its race is reported to the Casper team.
- **Residual false conflicts.** [I] A new payer cohort, which is a set of payers that share charge scopes, creates two cursors. Two new cohorts in one merge collide with a probability of about $`4/256 = 1/64`$. With $`m`$ new cohorts the expected number of colliding pairs is $`\binom{m}{2}/64`$. That is about 0.7 for $`m = 10`$, so the chance of at least one collision is about one half.
- **Order.** Without the v6 merge rule, sparse conflict graphs reach dev's exhaustive option search. The fix therefore lands second. [I]
- **Activation.** The change is in a genesis contract. It needs a fresh v6 genesis. A new legacy genesis gets the code but never calls it.

### 4.6 Footprint summary

| Item | Record | Casper files touched | v6 only | Status |
|---|---|---|---|---|
| G1 | DR-64 | none after the code motion of G2, cost-accounting code only | yes | approved, preparatory steps committed |
| G2 | DR-65 | `runtime.rs`, `runtime_manager.rs`, `replay_runtime.rs`, `block_creator.rs`, `errors.rs` | yes | approved, not started |
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
| At `f9bd3895d`, `casper/src/rust/util/rholang/acceptance.rs:2368` defines the block-assembly funding gate `admit_by_funding`. At HEAD no file under `casper/src` or `node/src` calls it. | `git grep` |
| At `f9bd3895d`, the multi-parent merge still ran dev's conflict search. `interpreter_util.rs:2756` calls `dag_merger::merge`, and `dag_merger.rs:2082` calls `conflict_set_merger::resolve_conflicts`, which is defined at `conflict_set_merger.rs:113`. | `git show` at `f9bd3895d` |
| Against its dev merge base, the branch changed its five merge files by 2,482 insertions and 1,726 deletions. It reworked the merger. It did not remove it. | `git diff --shortstat` |

**Why the realignment happened.** The user explained that the merge `1d325b996` was a necessary correction. The earlier branch had changed Casper beyond what cost accounting needs. The Casper team fixed the same problems in parallel, and the two versions conflicted. The review therefore separates changes that cost accounting requires from general Casper changes. Only the first kind can be proposed for reuse, and each one needs the user's approval.

### 5.1 Verdict for each expectation

| Expectation | What the branch did | Verdict | Key evidence at `f9bd3895d` |
|---|---|---|---|
| E1, merge logic | It reworked the merger into an exact-effect merge and removed no merge step. Non-test merge code grew from 7,490 to 8,299 lines (+10.8 %). [C] | **NOT MET** under every reading | Proposal reaches `dag_merger::merge` through `block_creator.rs:3580`. Validation reaches it through `validation_dispatcher.rs:400` and `interpreter_util.rs:895`. In a branch test, two funded charges by different payers on one shared cursor scope conflict. The merge rejects one of them after both ran (`casper/tests/util/rholang/monetary_cursor_branches.rs:142,207,215-216`). [C] |
| E2, user alternatives | No user-facing alternative exists. Inside one proposal, a state-bound loop sorts the candidates canonically and admits a prefix of each funding group. [C] | **NOT MET.** Partly met only inside one proposer's block, under the reading "funding decides the competition". | Group closure in `runtime.rs:647,693,814,844,1116`. The prefix rule departs from the simultaneous-arrival rule of P1 (P1:2271-2287). Across sibling blocks, the merge decides after both blocks ran (`dag_merger.rs:104-121`, `conflict_set_merger.rs:353`). [C][P] |
| E3, keep accepting blocks | Nothing lets a node accept a block before it executes the block. The proposer executes each candidate in a scratch runtime before admission. [C] | **NOT MET** for blocks. The weak reading, no node-wide lock on RSpace, already held on dev. | The state-bound loop (`runtime.rs:627-1190`). Every validator recomputes the merge (`interpreter_util.rs:895`) and replays the block (`interpreter_util.rs:1073`) before the block is valid. Replays share one permit (`runtime_manager.rs:147`, acquired at `:1283-1287`). [C] |
| E4, performance | No change targets merge cost, and the branch measured nothing. On v6 it adds merge and admission work. [C][I] | **NOT MET**, and not measured | Per-deploy exact state witnesses (`block_index.rs:196,273`), a projection check on every merge (`dag_merger.rs:1205-1211`), and the execution of every candidate before admission (`runtime.rs:627-1190`). [C] |

### 5.2 What the codex logs say

The review searched the prompt history of codex and its session logs from May to October 2026. [S]

- The review found no logged codex statement that all four expectations were met. The claim may exist outside the searched logs, for example in pull-request text.
- In the logged exchanges, codex rejected E1 and E4 and limited E3.
  - E1, on 2026-06-05: merge elimination "cannot mean all merge logic" (session `019e97d6`, line 718).
  - E1 and E2, on 2026-09-28: codex advised that the team "keep the residual multi-parent composition" and its conflict checks (session `01a0e3fe`, line 10252).
  - E3, on the same day: a node can receive or check more blocks while work runs. However, it "cannot certify a block" until the execution and replay evidence of that block is complete. The certification concerns the final state root and the settlement of the block (session `01a0e3fe`, line 10252).
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
- The proposer merges before it plays (`interpreter_util.rs:930-940`).
- Every validator recomputes the merge and compares the records (`interpreter_util.rs:423-532`).
- Every validated block gets a merge index when the shard allows more than one parent (`validation_dispatcher.rs:340-375`).
- The merger selects survivors after execution (`conflict_set_merger.rs:864-974`).
- `dag_merger.rs` and `conflict_set_merger.rs` differ from dev by +26 and −22 lines.

**Why the interim work does not close the gap.** [I] The v6 merge rule skips about 63 % of dev's merge lines on its fast path. It still reads execution traces, because conditions F4 to F7 need the event-log index. The eager index stays at 7.8 to 10.5 % of test work. Merging therefore remains "a separate analysis pass over execution traces", which P1:351-353 rules out.

**Recommended resolution.** Order-then-execute on v6 shards (§7). No state merge exists there. The executor's gate decides class S conflicts in the canonical order, and the canonical order serializes class D. The v6 merge rule retires on v6 shards in Phase 3, the order-then-execute phase of §10. Legacy shards keep dev's merger.

### 6.2 GAP-E2: users decide which alternatives they want

**Paper.** [P]
- Competing deployments "are two proof obligations competing for the same linear hypotheses, and at most one can succeed" (P1:348-351).
- Deployments with different signatures "cannot conflict" (P1:356-359). Deployments with the same signature are "serialized by the linear proof" (P1:360-363).
- Arrival in sequence: the first is accepted, the second is rejected before any step (P1:2245-2269). Simultaneous arrival: both are rejected (P1:2271-2287).
- A funding slot is "Open: anyone who deposits tokens on $`\mathit{slot}`$" (P1:1582-1583). The transition to self-funding "is a resource-exhaustion event, not a policy decision" (P1:1621-1622).
- Choice inside one deployment: acceptance holds the largest branch, and settlement refunds the rest (P1:2129-2152).

**Code at HEAD.** [C]
- At HEAD, no production path runs the static analysis (§3.6, note on DR-15).
- The proposer selects at most one offer per block (`block_creator.rs:1910-1916`).
- The global cost-cursor lock makes charged offered siblings conflict (`SystemVault.rho:52-56, 145-167`).
- The merger picks the survivor after both deploys ran (`conflict_set_merger.rs:864-974`).

**Why the interim work does not close the gap.** [I]
- G1 and G2 apply P1's gate and arrival rules inside one block only.
- Across validators, the v6 merge rule picks the survivor after execution by its order key $`K`$. Neither the signer's funding nor an arrival order picks it.
- The interim keeps one of two fully funded same-signer siblings (decision D-7, same-signer siblings under (a), §11). P1:360-363 serializes them, and both can succeed when the purse funds both.

**How a user expresses alternatives under cost accounting.** [I]
- Funding defines the alternatives. A user funds one slot with enough for one deployment and points every alternative at that slot. The funding check runs per located surface (P1:586-591).
- Arrival order picks the winner (P1:2245-2269). Simultaneous arrival rejects both (P1:2271-2287).
- A choice by a condition belongs inside one deployment as branches (P1:2129-2152).
- No new envelope field is necessary.

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
- A v6 block plays one envelope (`runtime.rs:359`), and the producer replays it (`runtime.rs:574-582`).
- More than one envelope is an error (`runtime.rs:641-650`, `runtime_manager.rs:1737-1741`).
- The legacy loop plays each deploy to termination (`runtime.rs:732-753`).
- Validators merge and replay a block before they accept it (`interpreter_util.rs:400-555`).
- A node replays one block at a time (`runtime_manager.rs:211-218, 1264-1267`).
- Each native frontier forms one conflict component (`deterministic_reduction.rs:1068-1072, 1120-1122`).

**Why the interim work does not close the gap.** [I]
- In execute-then-merge, the block content includes its post-state root (`CasperMessage.proto:597-605`). A validator can only check that root by execution. Acceptance of a block therefore waits for execution.
- A proposer needs the merged parent state before it can execute (`interpreter_util.rs:930-1005`).
- The v6 merge rule changes how the merge decides, not when it runs.
- A parallel executor shortens execution but does not decouple acceptance from it.

**Recommended resolution.** Order-then-execute (§7). Consensus accepts a block by a structural validity predicate, as P1:1708-1714 states. Execution follows finality and never blocks consensus.

### 6.4 GAP-E4: the performance claim

**Paper.** [P]
- Throughput today is "bounded by the execution time of the slowest deployment in the queue" (P1:281-282).
- The new model admits "as many deployments as there are non-conflicting token supplies" (P1:341-342).
- P2 attributes the gain to "*protocol fusion*" of fuel acquisition into single transitions (P2:856-865).
- No paper measures F1r3node.

**Code and data at HEAD.** [M] §9 gives the data. In short, the merge dominated block cost in the August soak. A code comment records that the parents-post-state stage "dominates block cost in sustained-load soaks" (`metrics_constants.rs:125-128`). After dev's fixes, the visible costs are the per-block reset, the replay and the checkpoint cycle.

**Why the interim work does not close the gap.** [I] The v6 merge rule removes the adjudication, about 1 to 1.5 % of test work. The eager index and the reset, replay and checkpoint cycle stay.

**Recommended resolution.** Measure first (§9.3). Then build order-then-execute, which removes the most work and takes replay and lock waits off the consensus path.

### 6.5 GAP-i: explicit transaction boundaries

**Paper.** "The for-comprehension is the transaction primitive ... A deployment may contain multiple transactions" (P1:318-325). [P]

**Code at HEAD.** [C] The native meter charges each COMM (DR-32). Funding admits only the sources of the envelope itself (`family_selection.rs:163-186`).

**Gap.** A continuation that one signer stored cannot draw on that signer's purse when another signer triggers it. Multi-deployment transactions "via shared signature channels" (P1:323-325) therefore fail.

**Recommended resolution.** Finish G1 and G3 (§4). No architecture change is necessary. Every candidate design needs them.

### 6.6 GAP-ii: concurrent acceptance

**Paper.** "RSpace can accept deployments *at any time* ... Multiple deployments can be active simultaneously" (P1:327-334). [P]

**Code at HEAD.** [C] One offer per block (`block_creator.rs:1910-1916`). One replay at a time (`runtime_manager.rs:211-218`).

**Why the interim work does not close the gap.** [I] G2 accepts several offers in one block. Across blocks, acceptance still waits for the merge and the replay.

**Recommended resolution.** Order-then-execute for acceptance (§7). The parallel batch executor for execution (§7.10).

### 6.7 GAP-iii: throughput

**Paper.** "the F1R3Node can accept and begin executing deployments in parallel" (P1:338-339). [P]

**Code at HEAD.** [C] Play and replay are sequential (`runtime.rs:738-753`, `runtime_manager.rs:1264-1267`).

**Why the interim work does not close the gap.** [I] G2 runs the members of a block one after another in canonical order.

**Recommended resolution.** The parallel batch executor (§7.10). Independent members run at once, and the result equals the sequential canonical execution.

### 6.8 GAP-iv: merge via signatures and cost accounting

**Paper.** "Merging is no longer a separate analysis pass over execution traces --- it is a consequence of the signature and cost-accounting structure of the calculus itself." (P1:351-354) [P]

**Code at HEAD.** [C] The merger searches for survivors after execution (`conflict_set_merger.rs:864-974`).

**Why the interim work does not close the gap.** [I] Inside a block, G1 and G2 decide conflicts by the linear proof. Across validators, the v6 merge rule still analyzes traces after execution.

**Recommended resolution.** Order-then-execute (§7). The gate decides class S in the canonical order. Class I composes, and with the parallel executor it also runs in parallel. The canonical order is the "natural serialization order" of class D (P1:363-366).

### 6.9 Gap matrix

The matrix rates each candidate design against the expectations and against the paper items (i) to (iv). [I]

| Design | E1 merge logic | E2 user alternatives | E3 keep accepting blocks | E4 performance | (i) transaction boundaries | (ii) concurrent acceptance | (iii) throughput | (iv) merge via signatures |
|---|---|---|---|---|---|---|---|---|
| (a) v6 merge rule, G1, G2 | Partial | Partial | Unmet | Marginal | Met with G1 and G3 | Partial | Unmet | Partial |
| (b) parallel batch executor, G1, G2, lock fix | Unmet | Partial | Partial | Partial | Met with G1 and G3 | In a block | In a block | In a block |
| (c) order-then-execute, sequential executor | Met on v6 | Met | Met | Largest, not measured | Met with G1 and G3 | Partial | Unmet | Met |
| (c)+(b) | Met on v6 | Met | Met | Largest, not measured | Met with G1 and G3 | Met | Met | Met |
| (d1) purse lanes | Partial | Met for funding | Unmet | Small | Met with G1 and G3 | Partial | Unmet | Partial |
| (d5) single-parent v6 | Mostly | Unmet | Unmet | Mixed | Met with G1 and G3 | Unmet | Unmet | Unmet |

### 6.10 Related gaps outside the four expectations

Two further gaps affect every design. Both must close before a v6 network carries production load.

**Host-work caps and the Embers initializer.** Embers is the F1R3FLY blockchain API that generates and deploys Rholang contracts ([real-world applications](../../rholang/19-real-world-applications.md)). The Embers testnet log initializer is a fixture in `offered_funded_api_test.rs:794`. Two tests run it. [M]

| Test | Committed caps | Provisional caps |
|---|---|---|
| `registry_initializer_is_funded_on_a_fresh_genesis` (`offered_funded_api_test.rs:987`) | Fails: "host work budget is rejected for verification operations: usage 20555630, requested 1" | Passes |
| `another_signer_initializes_a_registry_environment_after_a_registry_insert` (`offered_funded_api_test.rs:1025`) | Fails: "host work budget is rejected for verification operations: usage 20550481, requested 1" | Passes |

- **Source.** The test-suite runs of 2026-10-09 for work item D-F2. That item is the reuse of the reducer's measurement of each introduction (DR-118). The logs are `target/verification/d-f2/suite-committed.log` (lines 929-981) and `suite-provisional.log` (lines 933, 940). These local logs are not in the repository.
- **The two cap sets.** The committed caps are the protocol-6 host-work limits in `cost_protocol_limits.rs` and `production_limits.rs` at HEAD. The provisional caps are a candidate set of larger limits that the lane tests but has not committed. They change seven limit values in the same two files.
- **Limit of this evidence.** Both errors name the verification-operations dimension of the host-work budget. This document does not identify the smallest limit that the two tests need.
- **Why it matters for Casper.** The protocol-6 host-work caps are consensus constants. Play and replay must reject the same work. A shard must therefore fix them before activation.
- **Resolution.** Finish phase D of epic 8946, an internal phase of that epic. That work removes the last copies and repeated source builds from replay and play. Then measure the Embers workloads and settle the caps. Commit them with the activation of the v6 genesis.

**Settlement log-channel continuations.** [C][I] A vault owner can register a log channel (`SystemVault.rho:551-571`). Deposits and settlement steps send to it (`MakeMint.rho:55-88, 130-174`). System deploys run as the system payer under `Cost::unsafe_max()` (`runtime.rs:1724-1736`). A user continuation that fires at settlement is therefore neither metered nor bounded.

Under the parallel executor, such a loop stalls a worker. Under order-then-execute it stalls the executor. Meter and bound these continuations before Phase 2 (the parallel executor) or Phase 3 (order-then-execute) of §10.

---

## 7. The recommended architecture: order-then-execute for v6 shards

### 7.1 Overview

Order-then-execute separates two jobs that Casper does together today. [I]

1. **Consensus orders.** Blocks carry signed envelopes that the proposer has not executed. Validators accept blocks by a structural predicate. Fork choice and finality work as today.
2. **Execution follows.** When the LFB advances, a deterministic executor linearizes the newly finalized blocks. It applies P1's gate before each envelope, executes, settles, and records the root.

This is the classic order-execute split of state-machine replication [Schneider 1990]. Calvin uses a deterministic order before execution for the same reason [Thomson et al. 2012]. Execute-then-merge resembles the execute-order-validate pattern of Hyperledger Fabric [Androulaki et al. 2018]. [I]

![Sequence diagram of the recommended order-then-execute pipeline for v6 shards. The participants are the signing user, the proposer, the inclusion filter, the validators, finality, the linearizer, the executor and a root store. In the inclusion phase, the signing user sends a signed offered envelope. The proposer reads its newest executed finalized block E and its root r_E. The inclusion filter applies the DR-64 check at r_E, the DR-65 group rule and a per-block demand cap. The proposer publishes a block with the envelopes, the attestation (E, r_E) and the epoch validator set. In the validation phase, validators run only structural checks and add the block to the DAG. A green note says that consensus keeps accepting blocks and that no step waits for execution. In the finality phase, fork choice and the clique oracle work as today. The linearizer orders the newly finalized blocks by main-chain epochs. In the execution phase, the executor visits each envelope in canonical order. It skips repeats and envelopes outside their window. It applies the P1 gate. An accepted envelope is held, executed and settled, and its fee goes to the including proposer. A rejected envelope has no state change and consumes no tokens. The executor runs the system work of each block and records the executed roots. In the attestation phase, the next block attests a newer root. Validators check the attestation when their own execution reaches that block. A red note says that a mismatch is signed slashing evidence and never changes block validity after the fact. A legend explains the colours. It also defines P1, the P1 gate, the supply and the demand of a signature lane, DR-64 and DR-65. It defines E and r_E, e, M with mp(M) and past(M), LMD-GHOST, LFB, DAG, v6 and CloseBlock.](diagrams/consensus-proposal-order-then-execute.svg)

*Source: [`diagrams/consensus-proposal-order-then-execute.puml`](diagrams/consensus-proposal-order-then-execute.puml). Render it with `./render.sh consensus-proposal-order-then-execute.puml` in `docs/casper/theory/diagrams/`.*

The whole design is one literate program with five parts. Sections 7.2 to 7.7 refine each part.

```text
⟨order-then-execute on a v6 shard⟩ ≡
  ⟨propose without execution⟩                 -- §7.2, §7.4
  ⟨validate a block structurally⟩             -- §7.3
  ⟨linearize the newly finalized blocks⟩      -- §7.5
  ⟨execute the canonical order⟩               -- §7.6
  ⟨check attestations when execution reaches them⟩   -- §7.7
```

### 7.2 Block content

A v6 block keeps every consensus field of today: the sender, the sequence number, the signature, the justifications and the parents (`CasperMessage.proto:290-302, 329-334`). Its body changes meaning. [I]

| Field today | Meaning today | Meaning on a v6 shard under order-then-execute |
|---|---|---|
| `deploys` (`CasperMessage.proto:556`) | Executed deploys with their event logs | Accepted envelopes in block order, without event logs |
| `state.preStateHash`, `state.postStateHash` (`:598-599`) | The block's own merged pre-state and post-state | The attestation $`(E, r_E)`$: the newest finalized block $`E`$ that the proposer has executed, and its executed root $`r_E`$ |
| `state.bonds` (`:603`) | Bonds of the block's own post-state | The validator set of the block's epoch (§7.8) |
| `rejectedDeploys`, `appliedFromScope`, `mergeBase` (`:559-574`) | Merge results | Empty. Validity requires them to be empty. |
| `systemDeploys` (`:557`) | Executed system deploys | Slash records to execute at the block's position. CloseBlock work is implicit. |

The proposer does not execute. It reads its newest executed root and runs the inclusion filter (§7.4).

```text
⟨propose without execution⟩ ≡
  parents, justifications ← Casper fork choice and latest messages      -- unchanged
  (E, r_E) ← the newest finalized block that this node has executed, and its root
  V ← epoch_validator_set(parents)                                      -- §7.8
  batch ← ⟨inclusion filter⟩(pending envelopes in canonical order, r_E)
  publish block(parents, justifications, batch, attestation = (E, r_E), bonds = V)
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
  result: Valid or Invalid, with no merge, no execution and no replay
```

The signed phlo limit bounds the demand of an envelope (DR-64). The demand cap is therefore a syntactic check, in the sense of P1:1711-1714. [I]

**Deferred checks.** They need the executed state at $`E`$. A node runs them when its own executor reaches $`E`$ (§7.7). A failure produces signed slashing evidence. It never invalidates a block after the fact.

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

Four rules come from the papers. [P]

1. The gate precedes all execution of an envelope. Acceptance "precedes all execution" (P1:2303-2304).
2. A gate failure has no effect: "No state change occurs. No tokens are consumed." (P1:2211-2212).
3. Arrival in sequence across blocks follows P1:2245-2269. The canonical order defines "in sequence".
4. Simultaneous arrival applies only inside one block (P1:2271-2287, decision D-5).

The executor keeps the user's rule against escrow, in the reading of this document. [I] Holds live only inside the execution of one block, as they do in G2 today (DR-65). The no-escrow review exempts short-lived reservations inside one lexical execution (`cost-accounting-impl/funding-settlement-design-review.md:305-315`). The user must confirm that a hold across the members of one block is such a reservation (decision D-4).

### 7.7 Attestation of executed roots

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

### 7.8 Epoch-attested validator sets and the timing of slashing

**Today.** A block's bonds come from its own executed post-state (`CasperMessage.proto:603`, `block_metadata.rs:141-162`). The clique oracle reads them from the main parent (`clique_oracle.rs:202-220`). A slash zeroes a bond in the post-state of the block that executes it (slashing design, chapter 07). [C]

**Under order-then-execute.** Blocks have no own post-state, so the validator set comes from attested state. P1 updates validator sets at epoch boundaries. [P]

> "At each *epoch boundary* (when the set of staked validators is updated), the protocol mints a fresh supply of phlogiston for the validator's wallet, provided the validator has followed the rules of the protocol. If the validator commits a slashing offense, all remaining phlogiston is removed and no further phlogiston is minted." (P1:3036-3041)

The proposal: [I]

1. The validator set of epoch $`e`$ is the bond set in the executed state at the end of an earlier epoch. The lag in epochs is question Q-3.
2. Fork choice keeps its immediate exclusion of an equivocator. An equivocating block gets recorded evidence and the invalid-block effect (`validation_dispatcher.rs:611-633`). The estimator then filters the invalid latest message (`estimator.rs:3-23`). Detection is unchanged (`validation_dispatcher.rs:307-316`).
3. The clique oracle gets the same immediate exclusion: an equivocator in the target's view weighs zero (question Q-4). Today the bond becomes zero in the next post-state instead.
4. The slash record executes at its block's position. It removes the remaining phlogiston (P1:3040-3041). It moves the stake to adjudication (P1:3048-3050). Slash system deploys already carry a target activation epoch (`CasperMessage.proto:532-536`).
5. New bonds take effect at an epoch boundary.

A validator cannot check the `bonds` field of a block until its executor has reached the source epoch. It holds such a block, as it holds a block with a missing dependency (question Q-6). The demand cap must keep the execution lag below one epoch. [I]

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
| State | A deterministic function of genesis and the canonical order | new executor |

The rule asks for four properties. [I]

- **Secure.** The CBC safety argument concerns agreement on the block DAG under justifications and the safety oracle. The executor computes state from the agreed order, so it does not weaken that argument. The model `OrderThenExecute.tla` of Phase 3 must check this claim. The validator-set source changes, and Q-3 and Q-4 cover that change.
- **Auditable.** Every block lists its envelopes. Every executed root is attested and checkable. A wrong attestation is signed evidence.
- **Replayable.** Any node can replay the finalized order from genesis and reproduce every attested root.
- **A block chain.** Blocks remain signed, hash-linked and finalized by Casper.

The design therefore changes the interface between Casper and the replicated state machine. It does not change how Casper agrees on blocks [Schneider 1990]. [I]

### 7.10 The parallel batch executor inside a block

The executor of §7.6 can run the members of one batch in parallel. The result must equal the sequential canonical execution. [I]

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

**Basis.** Independent redexes commute (ST:367-383, ST:413-425). `DeterministicParallelReduction.v:204` proves `disjoint_parallel_schedule_refines_canonical` for disjoint schedules. Phase 2 generalizes it to batches. Block-STM uses the same idea, with a preset order and a result that equals sequential execution [Gelashvili et al. 2023]. [P][C]

**Prerequisites.** [I]
- The cost-cursor lock fix. Without it, every settlement writes one lock channel, and every pair of members conflicts.
- The metering of settlement log-channel continuations (§6.10).

Native scheduling inside one deploy is a separate source of parallelism. Today each native frontier forms one conflict component (`deterministic_reduction.rs:1068-1072, 1120-1122`). [C]

### 7.11 Properties to verify

| Property | Kind | Meaning |
|---|---|---|
| ExecutedRootAgreement | safety | Two honest executors that reach block $`b`$ record the same root. |
| GateBeforeExecute | safety | No step of an envelope runs before its gate accepts it. |
| AtMostOneOfCompeting | safety | Of two envelopes that the purse cannot fund together, at most one settles. |
| NoMergeRejection | safety | A v6 block carries no rejected record. |
| ValidationNeverExecutes | safety | Structural validation runs no Rholang. |
| BoundedLag | liveness | Under the demand cap, the executor stays within a bounded distance of the LFB. |

Failing TLA+ configurations must refute these properties when a guard is removed: ExecuteUnfinalizedOrder, LocalViewAuthoritative and NoDemandCap (§10).

### 7.12 Alternatives considered

| Alternative | Why it was rejected or deferred |
|---|---|
| (a) Execute-then-merge with the v6 merge rule only | E3 stays unmet. E1 and E2 are only partly met (§6). It is the interim for v6 shards until Phase 3. |
| (b) Parallel batch executor inside execute-then-merge | It shortens play and replay. Consensus still waits for execution, and every merge cost stays. It is kept as Phase 2 and becomes part of (c). |
| (d1) Purse lanes | Per-purse proofs compose (P2:1520-1527). Data-channel conflicts between lanes still need a merge. One lane can be censored or can stall. Rejected. |
| (d5) Single-parent v6 shards | It skips the eager index (`validation_dispatcher.rs:345`). It drops the concurrency that merging recovers (P1:290-294). A sole parent that lacks the floor still re-bases through the merge (`interpreter_util.rs:1284-1319`). Rejected. |
| Optimistic execution on the fork-choice chain | Lower latency than executing only the finalized prefix. It needs rollback. Deferred (decision D-3, the execution point and the order rule, §11). |
| Holds bound at inclusion | A hold that survives across blocks needs a reservation table. The user prohibited "regression to the retired escrow-based model" (`funding-settlement-design-review.md:13`). Not recommended (decision D-4). |
| Pure height-then-hash order of finality batches | Nodes with different finality steps diverge (§7.5). Rejected. |

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

---

## 8. Change analysis

### 8.1 Areas that must change

"Scope" says whether a change affects only v6 shards or touches code that legacy shards share. Legacy shards keep dev's path in every row.

| # | Area | Files | Change | Cost-accounting reason | Risk | Interaction with ongoing Casper work | Scope |
|---|---|---|---|---|---|---|---|
| 1 | G1 check before execution | `runtime.rs` and `runtime_manager.rs`, about 4 lines. The rest is cost-accounting code. | Run the DR-64 check before `evaluate_native_offered`. Validators recompute it. | P1:2188-2213 | Low | The stack-safe analyzer walks come from the F1R3Lang branches. | v6 only |
| 2 | G2 several offers per block | `runtime.rs` (about −230 / +25, moved out), `runtime_manager.rs`, `replay_runtime.rs`, `block_creator.rs`, `errors.rs` (about 105 lines) | An ordered batch with DR-65 groups | P1:2271-2287, P1:327-334 | Low, a v6 seam | None known | v6 only |
| 3 | G3 installer funding | `family_selection.rs:163-186` | The installer's purse is eligible for its stored region | P1:1557-1587 | Low | None known | v6 only |
| 4 | Cost-cursor lock fix | `SystemVault.rho:52-167` | 256 bucket locks on cursor creation only | P1:333-334 | Low. It needs a fresh v6 genesis. | The vault-creation race stays with the Casper team. | Genesis contract, called on v6 only |
| 5 | Interim v6 merge rule | `interpreter_util.rs`, `dag_merger.rs`, `conflict_set_merger.rs`, new `costacc/v6_merge` | Fast-path composition and one ordered pass | P1:344-366 | Medium | Dev reworked the merger in August and September, for example `cdb85d1e4`. | Shared files, v6 behavior |
| 6 | Settlement log channels | `SystemVault.rho:551-571`, `MakeMint.rho:55-174`, `runtime.rs:1724-1736` | Meter and bound continuations that fire at settlement | "every communicating process must engage the cost layer" (P1:2389-2391) | Medium | Dev's legacy precharge and refund likely share the hole. | Shared genesis contracts |
| 7 | Parallel batch executor | `runtime.rs`, `runtime_manager.rs`, `deterministic_reduction.rs` | Independent members at once, with the sequential result | P1:327-342 | Low, the roots equal the sequential roots | EPIC-021 measures the replay lock. It is the task-board epic on the root cause of issue #24, the finalization failures of the soak tests. | v6 only |
| 8 | Block body meaning | `CasperMessage.proto:554-575, 597-605` | Envelopes, the attestation and the epoch validator set | P1:1708-1714 | High | Any work on the block format | Shared message, v6 meaning |
| 9 | Proposal without execution | `block_creator.rs`, `interpreter_util.rs:893-1030` | Build blocks from the inclusion filter, with no merge and no play | P1:2303-2304 | High | None known | v6 only |
| 10 | Validation without merge or replay | `validation_dispatcher.rs:84-334`, `interpreter_util.rs:400-555` | Structural validity and deferred attestation checks | E3, P1:1708-1714 | High | Overlaps EPIC-021, which measures replay and checkpoint waits | v6 only |
| 11 | Executor driven by finality | `finalization_runner.rs:351-560`, `block_dag_key_value_storage.rs:1574-1640`, `runtime_manager.rs` | Linearize and execute each newly finalized set | P1:327-334 | High | EPIC-021 studies finalization latency. | Shared hook, v6 executor |
| 12 | LFB containment | `floor.rs:615-638` | DAG containment replaces state containment, because v6 has no merge rejections | P1:351-353, §6.1 | High, the finalized-floor proofs | The finalized-floor dossier | Shared, v6 branch |
| 13 | Validator set and slashing timing | `clique_oracle.rs:202-253`, `block_metadata.rs:141-162`, `estimator.rs`, PoS reads | Epoch-attested bonds, and equivocators weigh zero at once | P1:3036-3043 | High | Pull request (PR) #480 changed the finality weights (`clique_oracle.rs:222-253`). | Shared |
| 14 | State sync, deploy status, exploratory reads | node API, `casper/src/rust/engine/lfs_*_requester.rs` | Read from attested roots. Add the statuses included, finalized and executed. | Not in the papers. It follows from row 8. | Medium | API clients such as Embers | Shared |
| 15 | Recovery and the rejected-deploy buffer | `block_creator.rs`, DR-116 | Idle on v6, because there are no merge rejections | P1:351-353, §6.1 | Medium | None known | v6 only |
| 16 | Soak claims and models | `MergeAccounting.tla`, and the eight Casper soak claims CLAIM-CASPER-SOAK-001 to 008 in `docs/claims/` | New v6 profiles | Follows from rows 8 to 13 | High | The EPIC-021 soak harness | Shared |
| 17 | Host-work caps | `cost_protocol_limits.rs`, `production_limits.rs` | Settle the protocol-6 caps | Not in the papers. Replay must reject the same work (§6.10). | Medium | None known | v6 only |

### 8.2 Change map

![Component diagram of the Casper change map for order-then-execute on v6 shards, read from left to right. Group 1 is the unchanged Casper core in indigo. It holds signed blocks and justifications, fork choice, equivocation detection and the clique-oracle decision rule. Group 2 is shared code that gets a v6 branch, in amber. It holds the block body for v6, the epoch validator set, the finality hook, LFB containment, and the API and state sync. Group 3 is new v6-only machinery in green. It holds proposal without execution, structural validation, the linearizer, the executor, the parallel batch executor and the attestation check. Group 4 holds the cost-accounting modules, which are designed but not yet implemented. G1, G2, G3 and the cost-cursor lock fix are teal. The v6 merge rule is salmon, and it retires on v6 at Phase 3. Group 5 is dev's legacy path in grey. It holds the merger and the replay before acceptance. Each element carries its risk: high, medium or low. Arrows show the data flow from the core and the finality hook through the linearizer and the executor to the attestation and the block body. A legend explains the colours and the risk marks. It also defines v6, G1 to G3, DR-64, DR-120, pgmcp bug 11004, the P1 gate, Phase 3, LFB, LFS, API, DAG and LMD-GHOST.](diagrams/consensus-proposal-change-map.svg)

*Source: [`diagrams/consensus-proposal-change-map.puml`](diagrams/consensus-proposal-change-map.puml). Render it with `./render.sh consensus-proposal-change-map.puml` in `docs/casper/theory/diagrams/`.*

### 8.3 Interaction with ongoing Casper work

- **EPIC-021, the issue #24 root cause** (`docs/ToDos.md:74-106`). Order-then-execute removes the replay and the per-block reset and checkpoint from the consensus path of v6 shards. Legacy shards and the v6 executor still need the roots-lock fix `0ef0966c6`. Soak comparisons must separate v6 and legacy profiles. [I]
- **PR #480, the fix of issue 18** (`docs/ToDos.md:268`). It excludes silent bonded validators from the clique-oracle weight (`clique_oracle.rs:222-253`). Row 13 changes the weight source on v6 shards. The two changes must compose. [C][I]
- **Dev's merger rework.** For example `cdb85d1e4` resolves rejection groups independently. The v6 merge rule sits below dev's dispatcher and keeps dev's signature for its callers, so the conflict is medium. [C][I]
- **The finalized-floor and slashing dossiers.** Rows 12 and 13 change premises of mechanized proofs. Row 12 changes the state containment of the floor. Row 13 changes the bond-zero part of the fork-choice exclusion theorem T-10. Both dossiers need new proof obligations for v6. [I]
- **Epic 8946.** Its fixed exclusions forbid changes to consensus, finality and merging. Order-then-execute therefore needs a new epic (decision D-9).

---

## 9. Performance analysis

### 9.1 Measured data and their limits

**Profile of two legacy multi-parent tests on dev.** [M] AMD uProf recorded hotspot profiles of dev `93e38575e` on 2026-10-09. The host was one AMD Ryzen Threadripper PRO 5975WX, pinned to 16 hardware threads. T1 is `hash_set_casper_should_compute_identical_post_states_across_validators_for_merge_blocks`. T3 is `a_co_witnessed_sibling_fork_must_adjudicate_and_advance`. The profiling plan also names the legacy test T2, `multi_parent_casper_should_allow_bonding`. Its profiles follow the HEAD table below. Rayon is the thread-pool library of the node, and its idle threads spin. "Work" is sampled CPU time minus that rayon idle spin.

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
- The HEAD profiles follow in the next paragraph.
- The raw reports are local artifacts and are not in the repository.

**Profiles of HEAD.** [M] AMD uProf recorded T1 and T3 on HEAD `17e07307f` on 2026-10-09, with the same host and pinning. It also recorded the offered test T4, `a_third_signer_inserts_a_version_after_a_merge_with_a_writers_branch`, on HEAD. The report step of each HEAD profile needed 27 to 29 gigabytes of memory, so it ran under a cap of 32 gibibytes. The profiles of T2 finished later, and the paragraph after the table gives them.

| Inclusive share of work | T1, dev | T1, HEAD | T3, dev | T3, HEAD | T4, HEAD (offered) |
|---|---|---|---|---|---|
| Work, sampled CPU minus rayon idle spin | 9.30 s | 11.19 s | 9.12 s | 10.50 s | 19.07 s |
| Merge index, `block_index::new` | 10.5 % | 10.2 % | 7.8 % | 7.0 % | 2.9 % |
| `compute_parents_post_state` | 1.2 % | — | 1.8 % | 1.4 % | — |
| `dag_merger::merge` | 1.1 % | — | 1.6 % | 1.0 % | — |
| Replay, `replay_compute_state` | 5.4 % | 4.6 % | 7.5 % | 6.1 % | 8.7 % |
| Producer self-replay, `certify_offered_draft` | — | — | — | — | 7.2 % |
| Play, `compute_deploys_checkpoint` | 3.4 % | 2.7 % | 4.9 % | 3.7 % | — |
| Rholang reduction (dev `DebruijnInterpreter`, HEAD `ReducerCore`) | 34.2 % | 23.9 % | 27.9 % | 21.1 % | 34.0 % |
| RSpace produce and consume | 10.2 % | 7.6 % | 7.8 % | 7.6 % | 8.7 % |

A dash means that the report does not list the function. Either the function is below the report's cutoff of 400 entries, or it does not run on that path.

**T2 on dev and on HEAD.** [M] Work grew from 10.24 s on dev to 12.64 s on HEAD. The merge index took 11.6 % of work on dev and 10.8 % on HEAD. Replay took 5.3 % and 5.2 %, play took 2.3 % and 1.9 %, and Rholang reduction took 35.5 % and 24.2 %.

What the HEAD profiles show: [M][I]
- The merge takes about the same share of work on HEAD as on dev. In the offered-deploy test its share is smaller.
- HEAD does 15 to 23 % more work than dev on the three legacy tests. The extra work is outside the merge.
- In the offered-deploy test, replay and the producer's self-replay together take about 16 % of the work.
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

No metric separated failing sessions from passing sessions well. The AUC was at most 0.70 for any metric (`docs/ToDos.md:202`). Dev's roots-lock fix `0ef0966c6` is in HEAD. The verdict of its soak, run 37343966570, is not recorded. Two dev changes confound later comparisons (`docs/ToDos.md:267-269`). They are PR #480 and PR #523, which stops in-flight marker leaks in the block processor.

**Branch against dev.** [M] The integrated design review reported a local timing of the branch tests against dev. The branch was slower by a factor of 1.13 on merge-heavy tests and 1.15 on the rest. The slowdown was not in the merger. This document could not locate the artifacts of that timing, so the numbers are unverified.

**Reading.** [I] The merge dominated block cost in August. The stage metrics record the same observation: the parents-post-state stage "dominates block cost in sustained-load soaks" (`metrics_constants.rs:125-128`). After dev's fixes, the visible costs are the per-block reset, replay and checkpoint cycle. Both come from execute-then-merge with run-to-completion.

### 9.2 What each design and phase removes

| Design | Phase | Removes | Keeps |
|---|---|---|---|
| (a) | Phase 1b, interim merge (§10) | Adjudication, about 1 to 1.5 % of test work. The fast path also skips the settled-signature probes. | The index, 7.8 to 10.5 %. The reset, replay and checkpoint cycle. The v6 self-replay. |
| (b) | Phase 2 | Wall time of play and replay for independent members, up to 8.8 to 12.4 % of test work. CPU time stays. | Every merge cost. Consensus still waits for execution. |
| (c) | Phase 3 | The index and the parents post-state, about 10 to 12 % of test work (11.7 % for T1, 9.6 % for T3). The v6 self-replay. Executions of deploys that the merge would reject. Replay and lock waits leave the consensus path. | `reduce`, 27.9 to 34.2 %, much of it genesis. RSpace operations, 7.8 to 10.2 %. Rayon idle spin. |

No design touches the rayon idle spin, 58 to 63 % of sampled CPU. [M] Order-then-execute also ends the August feedback loop. Consensus no longer merges, so a finality lag cannot slow a merge. [I]

### 9.3 Measurements still needed, and the decision rule

| ID | Measurement |
|---|---|
| M1 | AMD uProf reports for HEAD tests T1 to T3 and for the offered test T4, `a_third_signer_inserts_a_version_after_a_merge_with_a_writers_branch`, one at a time. Done for T1 to T4 (§9.1). |
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

E1 to E3 require order-then-execute whatever $`\rho`$ turns out to be. [I]

---

## 10. Phased plan

The plan has five phases: Phase 0 (evidence), Phase 1 (acceptance core), Phase 1b (interim merge), Phase 2 (parallel executor) and Phase 3 (order-then-execute). In this document "P1" always names the paper, never a phase. Sizes are relative estimates: S (small), M (medium), L (large) and XL (extra large). Only Phase 0 and Phase 3 carry a duration estimate.

![Activity diagram of the phased plan. Two branches start in parallel. Phase 0, evidence, size S, covers the measurements M1 to M5 and gives evidence for or against E4 performance. Phase 1, acceptance core, size L and designed, covers G1, G2 and G3. Inside one block it meets (i) transaction boundaries, the P1 gate, E2 user alternatives and (iv) merge via signatures. Phase 1b, interim merge, size M to L and designed, follows Phase 1. It adds the v6 merge rule and then the cost-cursor lock fix, and it meets E1 merge logic in part. After the join, a prerequisite fix meters and bounds settlement log-channel continuations. A decision point follows, for the user, Greg and the Casper team. It asks if keep accepting blocks means acceptance before execution. The no branch is option B of decision D-1, the target for v6 shards. On it, Phase 2, parallel batch executor, size L, meets (ii), (iii) and (iv) inside one block, and E3 stays unmet. The recommended yes branch is option A of D-1. On it, Phase 2 builds the same executor, and Phase 3, order-then-execute, follows. Phase 3 is size XL, about four to nine engineer-months. It meets E1 to E4 on v6 shards. Each phase box names its verification artifacts. A legend explains the colours and defines every name that the boxes use.](diagrams/consensus-proposal-phase-plan.svg)

*Source: [`diagrams/consensus-proposal-phase-plan.puml`](diagrams/consensus-proposal-phase-plan.puml). Render it with `./render.sh consensus-proposal-phase-plan.puml` in `docs/casper/theory/diagrams/`.*

Every Rocq proof is axiom-free and has negative controls. Every TLA+ model has failing configurations that must refute their invariants. Every invariant becomes a property test.

| Phase | Content | Size | Prerequisites | Verification | Meets |
|---|---|---|---|---|---|
| Phase 0, evidence | M1 to M5 (§9.3) | S, 1 to 2 weeks | None | Preregistered experiments with A/A calibration | Evidence for or against E4 |
| Phase 1, acceptance core | G1 with the DR-64 holds, G2 as a batch executor with DR-65 groups and the held-capacity guard, G3 | L | The G1 chain of epic 8946 | `SameBlockFundingGroups.tla`: GroupAllOrNone, NoOverdraft, GateBeforeExecute, HeldCapacityGuard, DecisionPermutationInvariant. Controls: prefix admission, no held guard. Rocq group lemmas. `EndToEndAuthority.v` extensions. | (i) transaction boundaries, the gate of P1, E2 user alternatives and (iv) merge via signatures, inside one block |
| Phase 1b, interim merge | The v6 merge rule with C1 to C10, then the cost-cursor lock fix | M to L | G1 of Phase 1 | `V6MergeOrderedPass.v`, `CursorLinearity.v`, `CostCursorBuckets.v`, `V6MergeSerialization.tla`, `FeeCursorBranchMerge.tla`, `FeeCursorCells.tla`, with the C1 and C2 controls | E1 in part, bounded merges on v6 |
| Phase 2, parallel executor | The parallel batch executor, and the lock fix if it has not shipped | L | Phase 1, the log-channel fix, and Phase 1b or Phase 3 | Rocq: a parallel batch equals the sequential canonical batch. Controls: a shared-purse pair in parallel overdraws, an arrival-order commit is not canonical, a skipped re-run misses a COMM. TLA+ `ParallelBatchExecutor.tla` with SequentialEquivalence, NoOverdraft, HeldCapacityGuard, DeterministicRoot, and failing NoRevalidation, AuthorityKeyIgnored, NoHeldGuard. 256-seed property tests against a sequential reference. | (ii) concurrent acceptance, (iii) throughput and (iv) merge via signatures, inside one block |
| Phase 3, order-then-execute | §7 | XL, 4 to 9 engineer-months | Phase 1, the log-channel fix, the decisions of §11, a new epic | TLA+ `OrderThenExecute.tla`: three validators, permuted arrival orders, finality as an oracle, the invariants of §7.11, liveness BoundedLag, failing ExecuteUnfinalizedOrder, LocalViewAuthoritative, NoDemandCap. Rocq `CanonicalLinearization.v` (order deterministic and stable under prefix extension) and `OrderedGateSoundness.v` (the sequential gate is sound per purse, P2:1520-1527). Controls: arrival-order blocks diverge, a gate after execution strands funds. Three-node tests and a v6 soak. | E1, E2, E3, E4 on v6 shards |

The G2 member loop is the seam between the phases. Phase 1 builds it as an ordered batch API, Phase 2 parallelizes it, and Phase 3 feeds it finalized blocks. G1 stays in every phase. Under Phase 3 it becomes the inclusion filter and the executor's gate. [I]

The protocol-6 host-work caps of §6.10 must be settled before any v6 activation, whichever phase ships first.

---

## 11. Open decisions

These decisions belong to the user, Greg and the Casper team. Each row gives the options, the recommendation of this document, and what each option leaves unmet. [I]

| ID | Question | Options | Recommendation | What each option leaves unmet |
|---|---|---|---|---|
| D-0 | What does Greg mean? | Q1: does "keep accepting blocks" mean that consensus accepts a block before it executes it? Q2: does E1 count merge code that no longer runs, or the removal of after-the-fact trace analysis? Q3: who picks the winner among alternatives that a user submits separately, the canonical order or that submitting user? | Ask Greg first. | If Q1 is yes, only (c) among the designs of §6.9 meets E3. If Q1 is no, (b) is enough. If Q2 counts lines, (a) is enough for E1. If Q3 is "the submitting user", no paper mechanism exists (option B of D-6). |
| D-1 | The target for v6 shards | A: (c)+(b). B: (a)+(b), the v6 merge rule with the parallel batch executor. C: (a) alone. | A. It meets E1 to E4. It is XL and changes Casper's core. | B leaves E3 unmet and E1, E2 partial, and E4 marginal. C also leaves (ii) and (iii) unmet. |
| D-2 | The interim merge before (c) | A: the v6 merge rule with C1 to C10, then the lock fix. B: no interim rule. C: the lock fix alone on dev's resolver. | A. The user chose "implement what you can, cleanly". | A is thrown away on v6 at Phase 3: about 1,530 plus 2,600 lines and the models. B: until Phase 3, the global cost-cursor lock bug keeps rejecting every charged offer outside the main parent in v6 merges. C: groups collapse on shared mergeable channels (`conflict_set_merger.rs:900-912`). |
| D-3 | The execution point and the order rule in (c) | A: execute only the finalized prefix. B: execute optimistically on the fork-choice chain, with rollback. Order rule: main-chain epochs, or height then hash. | A first, with main-chain epochs (§7.5). | A: results wait for finality. B: rollback complexity. Height then hash over finality batches diverges between nodes. |
| D-4 | Where acceptance binds in (c) | A: at execution, as P1 states. A2: A, plus a filter that counts the unexecuted demand of the past cone (§7.4). B: derived in-flight holds at inclusion. | A2. No hold crosses a block. The user must confirm that holds inside one block are exempt from the no-escrow rule (§7.6). | A: jointly overcommitted envelopes can fill blocks and then fail at no cost (P1:2208-2212, §7.4). A2: concurrent blocks can still overcommit a purse. B: a reservation table that the user's no-escrow rule forbids (`funding-settlement-design-review.md:171-176`). |
| D-5 | "Simultaneous arrival" in a DAG | A: only inside one block (DR-65). B: concurrent sibling blocks too. | A | A: alternatives in concurrent blocks resolve by the canonical order. B rejects many honest pairs. |
| D-6 | User alternatives | A: funding, slots and branches inside a deployment. B: a new envelope-level alternative group. | A. "D6 deferred" was an agent's default, not the user's decision. D6 was the label of this decision in the v6 merge-rule design report. | A: a user cannot rank the alternatives that the same user submits separately. B: not in the papers, size M, and a new consensus encoding. |
| D-7 | Two fully funded same-signer siblings under (a). §4.4 gives the full analysis. | A: keep one, the current default. B: redesign the cursor so that both survive. | A while (a) is an interim. Under (c), both execute in order. | A departs from P1:360-363. B is size M, and Phase 3 makes it unnecessary. |
| D-8 | The validator-set source under (c) | A: epoch-attested state (P1:3036-3043). B: a validator registry in consensus, outside RSpace. | A, with the lag of Q-3 | A: a new bond waits up to the lag. B: a new protocol element. P1 says that slashing, minting and stake-weighted voting are expressible as Rholang contracts (P1:3777-3780). |
| D-9 | Ownership | A: a new epic, owned with the Casper team. B: extend epic 8946. C: the Casper team alone. | A | B: the fixed exclusions of epic 8946 forbid consensus, finality and merge changes. C: the cost-accounting invariants lose their owner. |

---

## 12. References

### 12.1 Governing papers

These papers are F1R3FLY publications without a digital object identifier (DOI). The links point to the [publications repository](https://github.com/F1R3FLY-io/publications) at revision `0bf7817`, the revision of every line reference.

- **[P1]** L. G. Meredith. *Cost-Accounted Rho Calculus: A Spectral Decomposition of Phlogiston.* F1R3FLY.io, May 2026. [`cost-accounting/cost-accounted-rho.tex`](https://github.com/F1R3FLY-io/publications/blob/0bf7817/cost-accounting/cost-accounted-rho.tex).
- **[P2]** L. G. Meredith. *Continued Interactive GSLTs and the Cost Endofunctor: A construction one level up from cost-accounted Rholang* (revised: wrapping by construction). F1R3FLY.io, May 2026. [`cost-accounting-as-monad/continued-gslt-cost-v2.tex`](https://github.com/F1R3FLY-io/publications/blob/0bf7817/cost-accounting-as-monad/continued-gslt-cost-v2.tex).
- **[ST]** L. G. Meredith. *Spacetime from Cost: A functor from cost-accounted ciGSLTs to measured causal sets.* F1R3FLY.io, June 2026. [`cost-spacetime/spacetime-functor.tex`](https://github.com/F1R3FLY-io/publications/blob/0bf7817/cost-spacetime/spacetime-functor.tex).

### 12.2 External works

Each DOI below was checked against Crossref and resolves at doi.org to the cited work.

- **[Androulaki et al. 2018]** E. Androulaki, A. Barger, V. Bortnikov, C. Cachin, K. Christidis, A. De Caro, D. Enyeart, C. Ferris, G. Laventman, Y. Manevich, S. Muralidharan, C. Murthy, B. Nguyen, M. Sethi, G. Singh, K. Smith, A. Sorniotti, C. Stathakopoulou, M. Vukolić, S. Weed Cocco, J. Yellick. "Hyperledger Fabric: A Distributed Operating System for Permissioned Blockchains." *Proceedings of the Thirteenth EuroSys Conference*, 2018, pp. 1-15. DOI: [10.1145/3190508.3190538](https://doi.org/10.1145/3190508.3190538).
- **[Bernstein 1966]** A. J. Bernstein. "Analysis of Programs for Parallel Processing." *IEEE Transactions on Electronic Computers* EC-15(5), 1966, pp. 757-763. DOI: [10.1109/PGEC.1966.264565](https://doi.org/10.1109/PGEC.1966.264565).
- **[Gelashvili et al. 2023]** R. Gelashvili, A. Spiegelman, Z. Xiang, G. Danezis, Z. Li, D. Malkhi, Y. Xia, R. Zhou. "Block-STM: Scaling Blockchain Execution by Turning Ordering Curse to a Performance Blessing." *Proceedings of the 28th ACM SIGPLAN Annual Symposium on Principles and Practice of Parallel Programming (PPoPP)*, 2023, pp. 232-244. DOI: [10.1145/3572848.3577524](https://doi.org/10.1145/3572848.3577524).
- **[Girard 1987]** J.-Y. Girard. "Linear Logic." *Theoretical Computer Science* 50(1), 1987, pp. 1-101. DOI: [10.1016/0304-3975(87)90045-4](<https://doi.org/10.1016/0304-3975(87)90045-4>).
- **[Kahn 1962]** A. B. Kahn. "Topological Sorting of Large Networks." *Communications of the ACM* 5(11), 1962, pp. 558-562. DOI: [10.1145/368996.369025](https://doi.org/10.1145/368996.369025).
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
  - DR-101: system residue is never charged to an earlier deployment
  - DR-113: exhaustion of the signed limit is a classified user failure
  - DR-114: native funded execution records and replays external-service replies
  - DR-115: a failed offered deploy keeps its committed settlement in the merge index
  - DR-116: recovery carries offered envelopes
  - DR-117: each COMM authority region is validated once
  - DR-118: an introduction reuses the reducer's measurement of its value
  - DR-119: one creation lock for each bucket replaces the global cost-cursor lock
  - DR-120: the interim v6 merge rule, a fast path or one ordered pass
  - DR-121: names that a receive body creates are dynamic for the funding analysis
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
