# Linear pre-execution cost estimate of an offered deploy

**Status.**

- The user adopted this design on 2026-10-10. No code implements it yet.
- pgmcp is the work tracker of this effort. Epic 8946 is its work item "Complete offered-funded production cost-accounting integration".
- G1, the funding check before execution, is a work item of epic 8946. Two implementation steps of G1 build this design. Step G1-4a adds the analysis to the `rholang` crate. Step G1-4b adds the check to the `casper` crate.
- This document is the reference for the amended linear cost estimate.

**Basis.**

- Code: HEAD, the commit `3e7af80e0` of the branch `feature/cost-accounted-rho`. A citation `file.rs:a-b` names lines a to b of that file at HEAD.
- Paper: P1, the paper *Cost-Accounted Rho Calculus* by L. G. Meredith, at publications revision `0bf7817`. A citation P1:a-b names lines a to b of its source file.
- Evidence tags: [C] code, [P] paper, [D] decision record, [I] inference.

## Contents

1. [Summary](#1-summary)
2. [Terms and notation](#2-terms-and-notation)
3. [The flow at a glance](#3-the-flow-at-a-glance)
4. [What P1, the decision records and the code require](#4-what-p1-the-decision-records-and-the-code-require)
5. [The linear analysis](#5-the-linear-analysis)
6. [How the check uses the estimate](#6-how-the-check-uses-the-estimate)
7. [Execution outcomes](#7-execution-outcomes)
8. [Settlement and the guard](#8-settlement-and-the-guard)
9. [The algorithm in literate form](#9-the-algorithm-in-literate-form)
10. [Worked examples](#10-worked-examples)
11. [Soundness argument](#11-soundness-argument)
12. [Verification plan](#12-verification-plan)
13. [What the estimate cannot see](#13-what-the-estimate-cannot-see)
14. [The client sizing rule](#14-the-client-sizing-rule)
15. [Relation to other documents](#15-relation-to-other-documents)
16. [Decision history](#16-decision-history)
17. [References](#17-references)

---

## 1. Summary

Cost-accounted Rholang charges every deploy in phlo, the cost unit of Rholang. In protocol version 6, every user deploy is an *offered deploy*. Its signed envelope carries a phlo limit, a phlo price and a list of funding sources. P1 requires the validator to compute the demand of a deployment "**Before** executing any part of the deployment" (P1:2191).

This document specifies how the node computes that demand. One linear analysis runs before execution, and it has two outputs.

1. **The estimate.** It gives the full cost of the deploy as raw counts, one count for each signature lane and each resource class. A *signature lane* is the part of the cost that one signature pays. The four *resource classes* are compute, introduction bytes, transfer bytes and trace bytes.
2. **Completeness.** The analysis also decides whether its estimate covers all cost. The estimate is *complete* when no dynamic logic can add cost. Eight rules, X1 to X8, state this condition so that every validator checks it in the same way.

A *COMM* is the communication event in which a receive meets one datum for each of its binds. The sends and the receive of a COMM are its *participants*. Compute counts COMMs. Introduction bytes measure what a send or a receive stores. Transfer bytes measure the data that a COMM delivers. Trace bytes measure the event log that a COMM commits.

Before execution, the check reserves funds on the funding sources. Each reservation is a *hold*.

- **Complete estimate.** The deploy holds exactly the estimated cost, times the phlo price, plus the fee. The fee is one atomic unit, the smallest unit of the token that the funding sources hold. This rule follows acceptance step 2 of P1 and decision record DR-64 (the funding check before execution).
- **Incomplete estimate.** The deploy holds its whole signed limit, times the phlo price, plus the fee. This rule is the margin rule of P1. DR-64 uses the signed limit as the margin.
- **Rejection.** The check rejects the deploy before execution if the holds do not fit the balances, or if the estimate exceeds the signed limit. Such a rejection has no effect and no charge.

After execution, *settlement* charges the measured use directly. There is no settlement floor. So refunds stay implicit, and offered state roots stay as they are today. A guard protects the funds that the block still needs. The guard is item 5 of DR-65 (same-block offers that share a purse).

Clients must sign a phlo limit at least as large as the estimate. The estimate counts each COMM once for each participant, so it can reach twice the realized use in compute, transfer and trace.

---

## 2. Terms and notation

This section defines the names, terms and symbols that later sections use.

### 2.1 Sources and labels

| Name | Meaning | Source |
|---|---|---|
| P1 | The paper *Cost-Accounted Rho Calculus* by L. G. Meredith, at publications revision `0bf7817` | `publications/cost-accounting/cost-accounted-rho.tex` |
| Greg | L. G. Meredith, the author of P1 and of the companion cost-accounting papers | The proposal, §1.2 |
| Greg's expectations | The four expectations that the user relayed from Greg: less merge logic, user alternatives, block acceptance without run-to-completion, and performance | The proposal, §1.2 |
| Casper | The consensus protocol of F1R3node and the crate that implements it | `casper/` |
| The proposal | The document `cost-accounting-consensus-proposal.md`, which states the remaining gaps for the Casper team | [The proposal](../cost-accounting-consensus-proposal.md) |
| DR-n | Decision record number n. DRs:a-b names lines a to b of the decision records file. | [Decision records](../cost-accounting-decision-records.md) |
| G1, G2, G3 | Work items of epic 8946. G1 is the funding check before execution (DR-64). G2 allows several offered deploys in one block (DR-65). G3 is installer funding of a stored continuation (DR-68). | The proposal, §2.1 |
| G1-4a, G1-4b | The two implementation steps of G1 that build this design: the analysis and the check | This document |
| G1-5 | A planned step that added a settlement floor. The user dropped it on 2026-10-10. | Section 16 |
| B1, B3, B5, C1, C2 | Labels of user decisions of 2026-10-10 on G1, G2 and G3. Section 16 states each one. | Section 16 |
| X1 to X8 | The eight completeness rules of the analysis | Section 5.3 |

### 2.2 Runtime and funding terms

| Term | Meaning | Source |
|---|---|---|
| Phlo | The cost unit of Rholang. It gates every deploy behind a token balance of a signature. | P1:145-147 |
| Offered deploy, envelope | A user deploy in the protocol-6 format. Its signed envelope holds the term, the signers, the funding sources, the signed phlo limit and the signed phlo price. | `offered.rs:11-21` |
| $`\mathrm{phloLimit}`$, $`\mathrm{phloPrice}`$ | The signed phlo limit and the signed phlo price of the envelope | DR-64 |
| SystemVault | The genesis contract that holds the purses | `SystemVault.rho` |
| Atomic unit | The smallest unit of the settlement asset that SystemVault holds | `phlo_schedule.rs:133-135` |
| Fee | The fixed charge of one atomic unit for each accepted offer | `acceptance.rs:22`, `phlo_schedule.rs:178-179` |
| Purse | One physical custody key of SystemVault | DR-65 |
| Source | One signed funding entry of the envelope. Its hold cap and its debit cap bound what it can hold and debit. The total exposure bounds all sources together. | DR-64 |
| Custody order | The canonical order of the sources by custody key | DR-64 |
| Original root $`R_0`$ | The authenticated state root that a deploy funds from | DR-64 |
| Hold | The amount that the check reserves on a source at $`R_0`$, before execution | DR-64, amended in Section 8.6 |
| Debit | The amount that settlement takes from a source | `sections.rs:173-180` |
| Settlement | The step after execution that charges the measured use and the fee | DR-64 |
| Member | One offered deploy of a block, in canonical order. Member $`i`$ funds from $`R_0`$ and executes from $`R_{i-1}`$, the root after member $`i - 1`$. | DR-65 |
| Signature | A name that pays for communications. P1 treats signatures as channel names. | P1:203-206 |
| Signed term | A term $`\{P\}_s`$ whose communications the signature $`s`$ pays | P1:218-222 |
| Funding signature | The signature that the signers of the envelope form. It pays for the work that no explicit signature covers. | `runtime.rs:165` |
| Signature lane | The part of the demand that one signature pays. Its key is the lane hash of the signature. | `delta_sigma.rs:92-104` |
| Deploy lane | The lane of the funding signature | `reduce.rs:1423-1441` |
| Region | One signed scope at run time. It has an instance identity, which is a Blake2b-256 hash, and a signature. | `authority.rs:404-424` |
| Authority | The set of regions that a send or a receive carries. The runtime merges regions that have equal instance identities. | `authority.rs:452-475` |
| Seal | The authority that a stored datum or continuation keeps for its later COMM | `reduce.rs:740-744` |
| RSpace | The tuple-space store of the node | `rspace++/` |
| Introduction | The storage of one send (a produce) or of one receive (a consume) in RSpace | [Byte accounting](vault-backed-byte-accounting.md) |
| Join | A receive with more than one bind. It fires only when every bind has a datum. | P1:965-971 |
| Resource class | One of the four native measurement dimensions: compute, introduction bytes, transfer bytes and trace bytes | `native_phlo_rules.rs:27-59` |
| Native meter | The meter that charges every resource class for every region during execution | `native_phlo_rules/execution.rs:162-220` |
| Weight | The phlo weight of one resource class in the adopted schedule | `phlo_schedule.rs:119-126` |
| Units of a lane | The number of Ground and Quote leaves of the signature of the lane | `phlo_execution.rs:477-491` |
| prost | The protobuf library of the node. Every byte size in this document is a prost-encoded length. | `byte_accounting.rs:72-74` |
| URN | A uniform resource name, such as `rho:io:stdout`. A `new` binds a URN to its value from the URN map. | `util/mod.rs:181-243` |
| URI | A uniform resource identifier. A `new` lists one URI for each URN that it binds. | `util/mod.rs:203` |
| Execution position | A place where the reducer runs a process. These are the top-level term and the bodies of signed terms, receives, `new`, match cases, `if` branches and bundles. | Section 5.2 |
| Binder kind | The kind of binder that a de Bruijn level comes from: a name of a `new`, a URN, an injection or a pattern variable. A de Bruijn level is the position of a binder on the stack of enclosing binders. | Section 5.2 |
| URN map | The map from each URN to its channel value. Play and replay build it in the same way. | `rho_runtime.rs:1997-1999`, `:2065-2068` |
| Host-work budget | A per-deploy budget of deterministic node work. Its limits are consensus constants. It charges no tokens. | [Host-work budget](../host-work-budget.md) |
| Bound source | The source of the scalar bound of the native meter: `Certificate` for a certified bound, or `SignedLimit` for the signed limit | DR-113 (exhaustion of the signed limit) |

### 2.3 Notation

| Symbol | Meaning |
|---|---|
| $`s`$ | A signature lane |
| $`k`$ | A resource class: $`\mathrm{compute}`$, $`\mathrm{intro}`$, $`\mathrm{transfer}`$ or $`\mathrm{trace}`$ |
| $`o`$, $`r`$, $`b`$ | A send, a receive, and one bind of a receive |
| $`x`$ | A channel. $`\mathrm{ch}(o)`$ and $`\mathrm{ch}(b)`$ are the channels of a send and of a bind. |
| $`\mathrm{binds}(r)`$, $`n_r`$ | The binds of receive $`r`$, and their number |
| $`\rho`$, $`\mathrm{lane}(\rho)`$ | A region, and the lane of its signature |
| $`\Lambda_o`$ | The region multiset of send $`o`$ |
| $`\Lambda^{\mathrm{I}}_r`$, $`\Lambda^{\mathrm{C}}_r`$ | The introduction regions and the COMM regions of receive $`r`$, as multisets |
| $`\mu_s(\Lambda)`$ | The number of regions of lane $`s`$ in the multiset $`\Lambda`$, counted with multiplicity |
| $`\lvert v \rvert`$ | The prost-encoded length of the value $`v`$, in bytes |
| $`\mathrm{msg}(o)`$ | The datum message of send $`o`$: its data, its random state and its seal |
| $`\mathrm{pat}(b)`$, $`\mathrm{cont}(r)`$ | The pattern of bind $`b`$, and the continuation of receive $`r`$ with its seal |
| $`\mathrm{body}(r)`$ | The body of receive $`r`$, the process that runs when the receive fires |
| $`\mathrm{ib}(o)`$, $`\mathrm{ib}(r)`$ | The introduction bytes of a send and of a receive |
| $`M(x)`$, $`T(x)`$, $`\mathrm{TR}(x)`$ | The channel tables: the largest datum message, the largest transfer and the largest trace on channel $`x`$ |
| $`v_o(s,k)`$, $`v_r(s,k)`$ | The contribution of one send or one receive to lane $`s`$ and class $`k`$ |
| $`V(P)`$, $`V(s,k)`$ | The raw-count vector of a term $`P`$, and the raw count of the whole deploy for lane $`s`$ and class $`k`$ |
| $`\sqcup`$ | The componentwise maximum of two vectors |
| $`w_k`$ | The weight of class $`k`$ in the adopted schedule |
| $`u(s)`$ | The units of lane $`s`$ |
| $`\Delta^{\mathrm{known}}_s`$ | The known demand of lane $`s`$, in phlo |
| $`B`$ | The estimate in phlo: the sum of the known demands |
| $`H`$ | The total hold of one offer, in atomic units |
| $`R`$ | The remainder that an incomplete estimate holds beyond its known demand |

---

## 3. The flow at a glance

![Activity diagram of the linear pre-execution cost estimate of one offered deploy, in six partitions. Partition 1 holds the inputs. The producer and every validator read the signed envelope and four consensus inputs. These are the schedule with its weights, the URN map, the host-work limits and the wallet snapshot at R0. In partition 2, the metered resolver binds the names of the deploy from the envelope seed. Partition 3 is the linear analysis. It gives every level a binder kind and sizes every send and receive. It counts COMMs and bytes for every region, with maxima at branches and joins as multisets. Then it computes the estimate B. Its decision asks whether rules X1 to X8 and the on-chain payer condition hold. The answer is its second output: the estimate is complete or incomplete. Partition 4 is the supply check at R0. If B exceeds phloLimit, the check rejects before execution with no effect and no charge. A complete estimate holds B times phloPrice plus 1 under the Certificate bound B. An incomplete estimate holds phloLimit times phloPrice plus 1 under the SignedLimit bound. The check places the known demand per lane and class, and it fills any remainder in custody order. If the holds do not fit, it rejects with the text insufficient phlogiston and no charge. Partition 5 is the execution. Use within the bound goes on to settlement. Exhaustion of the signed limit is a classified user failure that charges the billable work and the fee. A total use above B is a certificate failure with no charge. A host-work limit rejects with no charge. Partition 6 is settlement under the guard of DR-65 item 5. For each purse p in scope, the balance after execution minus the debit of member i must cover the holds of the later members j. If the guard holds, settlement charges the measured use plus the fee, with hold equal to debit. If it fails, the member is a charged user failure. Notes list what the estimate cannot see, the capacity cap of several offers per block and the interim rule B5. A legend explains the colours and defines every name.](../diagrams/linear-cost-estimate.svg)

*Source: [`../diagrams/linear-cost-estimate.puml`](../diagrams/linear-cost-estimate.puml). Render it with `./render.sh linear-cost-estimate.puml` in `docs/casper/theory/diagrams/`.*

The diagram has six partitions. Each step below describes one partition.

1. **Inputs.** The producer and every validator read the signed envelope and four consensus inputs. The inputs are the adopted schedule, the offered URN map, the consensus host-work limits and the wallet snapshot at $`R_0`$.
2. **Resolver.** The metered resolver binds the names that the reducer allocates when it runs the deploy (`lexical.rs:63-70`).
3. **Linear analysis.** One analysis computes the estimate and decides whether the estimate is complete (Section 5).
4. **Holds and the supply check.** The check rejects an estimate above the signed limit. Then it places the holds at $`R_0`$, or it rejects the deploy (Section 6).
5. **Execution.** The deploy runs under the estimate when the estimate is complete, and under the signed limit otherwise (Section 7).
6. **Settlement and the guard.** Settlement charges the measured use and the fee. The guard makes a member that spends needed funds a charged user failure (Section 8).

---

## 4. What P1, the decision records and the code require

### 4.1 What P1 requires

P1 states six requirements that the estimate must meet. [P] Acceptance step 2 refers to the abstract syntax tree (AST) of a deployment.

1. **Demand before execution.** "**Before** executing any part of the deployment, the validator computes the demand by static analysis of the deployment's AST and call graph" (P1:2191-2193).
2. **The exact demand where it exists, and a maximum where data choose.** Step 2 continues: "the exact demand $`\Delta_c(D)`$ for the data-independent fragment, or the conservative bound $`\Delta_c^{\max}(D)`$ ... where control flow is data-dependent" (P1:2193-2197). Here $`D`$ is a deployment, $`c`$ is its client, and $`\beta`$ ranges over the statically reachable branches of $`D`$ (P1:2138-2152):

   ```math
   \Delta_c^{\max}(D) \;=\; \max_{\beta \in \mathrm{Branches}(D)} \Delta_c(\beta).
   ```

   Acceptance needs the token supply $`\Sigma_c`$ of the client to satisfy $`\Sigma_c \geq \Delta_c^{\max}(D)`$. After execution, the refund is $`\Delta_c^{\max}(D) - \kappa`$, where $`\kappa`$ is the number of tokens that the run forced (P1:2148-2151).
3. **A margin for unknown demand.** Unresolvable dequotation terms "contribute an “unknown” demand, and the validator rejects unless the supply exceeds the known lower bound plus a configurable safety margin" (P1:2077-2080).
4. **Linear time.** "For a deployment whose call graph is statically known (no higher-order channel passing, no recursive dequotation chains), the funding proof obligation ... is decidable in time linear in the size of the deployment's AST" (P1:2062-2066).
5. **Commitment.** "Once accepted, those resources are committed and unavailable to other deployments." (P1:2301-2302).
6. **No partial funding.** "The token stack must supply *all* of these or *none* of them. Partial supply leads to partial execution, which is the vulnerability." (P1:1990-1992). P1 calls under-funding "a correctness failure" and over-funding "merely an inefficiency" (P1:2111-2125).

A rejected deployment gets the error "insufficient phlogiston" (P1:2210). P1 counts tokens, one for each signed layer that a run forces. It assigns no byte tariff. The byte tariff of the node is "a native F1R3node safety refinement" (`vault-backed-byte-accounting.md:17-18`). This design extends the demand of P1 to that refinement, as the user directed on 2026-10-10 (Section 16).

### 4.2 What the decision records require

The design rests on eight decision records. [D]

- **DR-5** (precharge and refund removed). "The acceptance-by-linear-proof model makes escrow precharge/refund unnecessary." (DRs:165). A hold is therefore a check, not an escrow transfer.
- **DR-64** (the funding check before execution). Before execution, the producer and every validator compute the known demand $`\Delta^{\mathrm{known}}_s`$ of each lane $`s`$ from the envelope and $`R_0`$ only. A construct that the analysis cannot prove contributes 0. The unprovable remainder is held in full. Holds that do not fit cause a rejection with no effect and no charge (DRs:3878-3898). Its verification obligations include "A static offer holds its exact demand" (DRs:3974-3976). Section 8.6 amends its items 4 and 6.
- **DR-65** (same-block offers that share a purse). Members execute in canonical order. A group of members that share a purse is rejected as a whole when its holds exceed the supply of one purse. Its item 5 states the held-capacity guard (DRs:4032-4040).
- **DR-67** (stack safety). The walks of the analyzer and of the resolver use explicit work stacks.
- **DR-101** (system residue). "A fired continuation runs its body with an empty authority" (DRs:8298-8299). Each unsigned send or receive in that body opens a region of the current payer. "The COMM authority merges the stored seals of all participants" (DRs:8311-8312).
- **DR-113** (exhaustion of the signed limit). The native meter names its bound source. Its outcome table maps each exhaustion to a class and an effect (DRs:12007-12012).
- **DR-118** (introduction measurement). An introduction reuses the reducer's prost measurement of its value. The charge formulas stay those of `byte_accounting.rs`.
- **DR-121** (names that a receive body creates). A `new` inside a receive body binds holes, because the reducer derives those names from the matched datum. A signature that names such a binder is dynamic authority.

### 4.3 What the native meter charges

The native meter defines the four classes and their charges. [C]

- **Classes.** Compute is measured in COMMs. Introduction, transfer and trace are measured in bytes (`native_phlo_rules.rs:27-59`).
- **Charge per region.** For each observation, the meter charges every region $`\rho`$ of its authority. The charge is $`w_k \cdot u(\mathrm{lane}(\rho)) \cdot q`$, where $`q`$ is the measured quantity (`native_phlo_rules/execution.rs:186-214`, `regions.rs:118-132`).
- **Units.** The units of a lane are the number of Ground and Quote leaves of its signature (`phlo_execution.rs:477-491`).
- **Bound.** The meter fails with `BoundExceeded` when the usage would pass the bound (`native_phlo_rules/execution.rs:274-287`).
- **Fee.** The acquisition charge is the fresh usage times the price, plus one atomic unit (`phlo_execution.rs:561-564`).

The byte charges come from `byte_accounting.rs`. Schedule version 1 sets every byte rate to 1 (`:23-27`). A produce introduction of a send $`o`$ charges (`:139-151`):

```math
\mathrm{ib}(o) \;=\; \lvert \mathrm{ch}(o) \rvert + \lvert \mathrm{msg}(o) \rvert + 64.
```

A consume introduction of a receive $`r`$ charges (`:153-167`):

```math
\mathrm{ib}(r) \;=\; \sum_{b \in \mathrm{binds}(r)} \bigl( \lvert \mathrm{ch}(b) \rvert + \lvert \mathrm{pat}(b) \rvert \bigr) + \lvert \mathrm{cont}(r) \rvert + 32 + 32\,n_r.
```

A COMM of receive $`r`$ with the matched sends $`o_1, \ldots, o_{n_r}`$ charges these transfer and trace bytes (`:296-319`):

```math
\mathrm{transfer} \;=\; \sum_{i=1}^{n_r} \lvert \mathrm{msg}(o_i) \rvert,
\qquad
\mathrm{trace} \;=\; 32 + 96\,n_r.
```

### 4.4 What today's analyzer does

The analyzer `static_authority_plan` already computes the compute class. [C] It runs one iterative walk, `signed_demand_par` (`delta_sigma.rs:353-578`, `:756-799`).

- It counts one potential COMM for every enclosing region of every send and receive (`:276-295`), and one for every signed bind (`:489-497`).
- It adds parallel parts and takes the componentwise maximum at `if` and `match` (`:516-542`, `:565-574`).
- It rejects persistent sends and receives, a dequotation `*x` in process position, dynamic authority and mixed signed binds (`:325-334`, `:464-504`, `:549-563`).
- It counts no bytes. It ignores peeks and method calls in process position.
- No production path calls it yet (the proposal, §3.6).

This design adds the three byte classes and the completeness decision to that walk.

---

## 5. The linear analysis

### 5.1 Inputs and outputs

The analysis is a pure function of five inputs. [C]

1. The envelope term with its normalizer environment (`runtime.rs:167-175`).
2. The seed that the envelope identity gives (`tools.rs:20-27`).
3. The adopted schedule, which gives the weights $`w_k`$.
4. The offered URN map (`rho_runtime.rs:1997-1999`, `:2065-2068`).
5. The consensus host-work limits.

The funding signature comes from the signers of the envelope (`runtime.rs:165`). The analysis has three outputs.

- The raw counts $`V(s,k)`$, the known demands $`\Delta^{\mathrm{known}}_s`$ and the estimate $`B`$.
- The completeness of the estimate. An incomplete estimate also reports the first rule that failed.
- The static signature of every lane, which the obligations need.

The planned interface of step G1-4a follows. The names are planned, and no code exists yet.

```rust
pub enum IncompleteReason {
    StaticPlan(UnprovableDemand),
    OnChainPayer,
    NonFreshChannel,
    EscapingName,
    OpenData,
    OpenPattern,
    ReceivedValueInIntroduction,
    PeekOrStack,
    MethodInProcessPosition,
    DataEvaluation,
}

pub enum Completeness {
    Complete,
    Incomplete(IncompleteReason),
}

pub struct CostEstimate {
    pub quantities: BTreeMap<(SigKey, usize), u64>,
    pub bound_phlo: u64,
    pub signatures: BTreeMap<SigKey, CostSignature>,
    pub completeness: Completeness,
}

pub fn linear_cost_estimate(
    resolved: &Par,
    funding: &Sig,
    schedule: &PhloScheduleV1<'_>,
    urn_map: &HashMap<String, Par>,
    host: &HostWorkBudget,
) -> Result<CostEstimate, InterpreterError> {
    todo!("step G1-4a")
}
```

The field `quantities` holds the raw counts $`V(s,k)`$, keyed by the lane key and the class index. The field `bound_phlo` holds $`B`$. An error means that the host-work budget ran out, and the check then rejects the deploy with no charge (Section 6.3).

### 5.2 Execution positions and binder kinds

An *execution position* is a place where the reducer runs a process. The execution positions are the top-level term and the bodies of signed terms, receives, `new`, match cases, `if` branches and bundles. Send data and quoted processes inside data are not execution positions. [C]

The analysis gives every de Bruijn level a *binder kind*. A de Bruijn level is the position of a binder on the stack of enclosing binders. The resolver rewrites only cost signatures, and its holes carry no kind (`lexical.rs:297-314`). So the analysis keeps its own kind for each level.

| Kind | What binds the level | Value in the sizing environment |
|---|---|---|
| Fresh | A simple name of a `new` at a resolving position | Its resolved private name, with a 32-byte identity |
| Fresh hole | A simple name of a `new` inside a receive body | A private name with a 32-byte placeholder identity |
| URN | A URI of a `new` that the URN map binds | Its value in the URN map |
| Injection | A URI of a `new` that an injection binds | The injected value |
| Receive pattern | A variable of a receive pattern | Poison |
| Match pattern | A variable of a match pattern | Poison |

A `new` pushes its simple names first. Then it pushes one level for each URI, in the order of `allocate_new_bindings`, which looks up the URN map before the injections (`util/mod.rs:181-243`). A URI level keeps its URN or injection kind at every depth. This holds inside a receive body too, where the resolver pushes only holes (`lexical.rs:598-608`). A receive or a match case pushes one pattern level for each variable that its patterns bind.

### 5.3 Completeness rules

The estimate is complete only if rules X1 to X8 and the on-chain payer condition all hold. The analysis checks every rule against binder kinds, never against values. [C][I]

| Rule | Condition | Reason |
|---|---|---|
| X1 | `static_authority_plan` of the resolved term and the funding signature returns `Ok`. | It excludes persistent sends and receives, `*x` in process position and dynamic authority (`delta_sigma.rs:756-799`). With X8, every receive then fires at most once and consumes one datum per bind. |
| X2 | At every execution position, every send and receive channel is a bound level of kind fresh or fresh hole. | A public, URN or injected channel can reach a stored contract or stored data. The body of a stored contract runs with an empty authority and charges the deploy lane (`reduce.rs:382-406`). |
| X3 | A level of kind fresh or fresh hole occurs only as such a channel or in a cost signature. | A name that never escapes cannot be held by stored state. This escape analysis supports the transfer bound. |
| X4 | Send data are closed: their free-variable set (`locally_free`) is empty. The analysis sizes them after the reducer's evaluation (Section 5.6). | Data sizes then do not depend on the run. Quoted processes in data stay allowed, because X2 and X8 apply to execution positions only. |
| X5 | Receive patterns have no free variable of an outer level. | Pattern sizes then do not depend on the run. |
| X6 | A receive-pattern or match-pattern level occurs only in `if` conditions and match targets of its own body. It never occurs inside a nested introduced value. | Received values then only choose branches, and branches take the maximum (P1:2193-2197). The poison check of Section 5.6 enforces this rule. |
| X7 | No bind is a peek, and the term has no cost stack. | A peeked datum can be delivered more than once. A cost stack brings retained acquisition. |
| X8 | No method call (`EMethodBody`) is among the top-level expressions of a term at an execution position. | The reducer runs the result of a method as a process with the current authority (`util/mod.rs:136-145`, `reduce.rs:1380-1390`). The existing walk checks only `EVarBody` (`delta_sigma.rs:549-563`). |

**On-chain payer condition.** If any signed source is an on-chain private-name payer, the estimate is incomplete. A source is an on-chain payer when no signer and no grant witnesses it (`direct_wallet_funding.rs:282-297`). Eligibility for such a purse needs an observed private-name authority (`family_selection.rs:50-67`), and rules X2 to X4 forbid a program that holds a stored private name.

### 5.4 Participants and their regions

Every participant carries a multiset of regions. The rules below follow the reducer. [C]

- **A send $`o`$.** $`\Lambda_o`$ holds the enclosing regions, one for each enclosing signed scope. When the enclosing authority is empty, $`\Lambda_o`$ holds one fresh region of the deploy lane instead (`reduce.rs:1423-1441`). The introduction and the COMMs of the send use $`\Lambda_o`$.
- **The introduction of a receive $`r`$.** $`\Lambda^{\mathrm{I}}_r`$ holds the enclosing regions, or one fresh region of the deploy lane when the enclosing authority is empty. Signed binds do not change it (`reduce.rs:1520-1531`). The reducer registers the consume introduction with this authority (`:1648-1657`, `:753-758`).
- **The COMMs of a receive $`r`$.** With signed binds, $`\Lambda^{\mathrm{C}}_r`$ holds the enclosing regions plus one region for each signed bind, and no deploy region. Without signed binds, $`\Lambda^{\mathrm{C}}_r`$ equals $`\Lambda^{\mathrm{I}}_r`$ (`reduce.rs:1532-1563`).
- **A receive body.** It runs with an empty authority (`reduce.rs:382-406`). So each unsigned send or receive in the body opens its own region of the deploy lane.

The analysis counts every region with multiplicity, because the meter charges every region. Two regions of one lane are different regions when their instance identities differ:

- Each signed bind of a join gets its own discriminator (`reduce.rs:1554`). So a join with two binds signed by $`s`$ carries two regions of lane $`s`$.
- A nested signed scope gets a new region from its own randomness (`reduce.rs:1972-1986`). A term with several parallel parts splits the randomness among them (`util/mod.rs:160-179`).

The runtime merges two regions only when their instance identities are equal (`authority.rs:452-475`). This happens for a nested signed scope of the same signature that is the only term of its parent body. That body reuses the randomness of its parent (`util/mod.rs:172-173`, `reduce.rs:1996`). A merge only lowers the charge. So a count of every syntactic region is a sound upper bound.

### 5.5 The demand vector

The analysis first builds three channel tables. A channel is a level of kind fresh or fresh hole, so each table is an array indexed by binder. The maximum of an empty set is 0.

```math
M(x) \;=\; \max\,\{\, \lvert \mathrm{msg}(o) \rvert \;:\; \mathrm{ch}(o) = x \,\}
```

```math
T(x) \;=\; \max\,\Bigl\{\, \sum_{b \in \mathrm{binds}(r)} M(\mathrm{ch}(b)) \;:\; r \text{ has a bind on } x \,\Bigr\}
```

```math
\mathrm{TR}(x) \;=\; \max\,\{\, 32 + 96\,n_r \;:\; r \text{ has a bind on } x \,\}
```

The sum in $`T(x)`$ runs over the binds of a receive as a multiset. A join with two binds on one channel counts the datum of that channel twice, because it consumes two data (`byte_accounting.rs:296-313`).

Each send $`o`$ contributes, for every lane $`s`$:

```math
\begin{aligned}
v_o(s,\mathrm{compute}) &= \mu_s(\Lambda_o), &
v_o(s,\mathrm{intro}) &= \mu_s(\Lambda_o)\,\mathrm{ib}(o), \\
v_o(s,\mathrm{transfer}) &= \mu_s(\Lambda_o)\,T(\mathrm{ch}(o)), &
v_o(s,\mathrm{trace}) &= \mu_s(\Lambda_o)\,\mathrm{TR}(\mathrm{ch}(o)).
\end{aligned}
```

Each receive $`r`$ contributes, for every lane $`s`$:

```math
\begin{aligned}
v_r(s,\mathrm{compute}) &= \mu_s(\Lambda^{\mathrm{C}}_r), &
v_r(s,\mathrm{intro}) &= \mu_s(\Lambda^{\mathrm{I}}_r)\,\mathrm{ib}(r), \\
v_r(s,\mathrm{transfer}) &= \mu_s(\Lambda^{\mathrm{C}}_r) \sum_{b \in \mathrm{binds}(r)} M(\mathrm{ch}(b)), &
v_r(s,\mathrm{trace}) &= \mu_s(\Lambda^{\mathrm{C}}_r)\,(32 + 96\,n_r).
\end{aligned}
```

The vector of a term follows its structure. For terms $`P`$ and $`Q`$, parallel parts add. A branch point takes the componentwise maximum $`\sqcup`$ of its branches. A receive adds its own contribution to the vector of its body. In the last two lines below, $`e`$ is the condition or the match target, and $`P_1, \ldots, P_\ell`$ are the $`\ell`$ case bodies.

```math
\begin{aligned}
V(P \mid Q) &= V(P) + V(Q) \\
V(\{P\}_{s}) &= V(P) \quad \text{(each participant of } P \text{ gains one region of lane } s\text{)} \\
V(\mathtt{new}\ x\ \mathtt{in}\ P) &= V(P) \\
V(o) &= v_o \\
V(r) &= v_r + V(\mathrm{body}(r)) \\
V(\mathtt{if}\ (e)\ P\ \mathtt{else}\ Q) &= V(P) \sqcup V(Q) \\
V(\mathtt{match}\ e\ \{ P_1, \ldots, P_\ell \}) &= V(P_1) \sqcup \cdots \sqcup V(P_\ell)
\end{aligned}
```

A bundle in process position has the vector of its body. The raw count of the deploy is $`V(s,k) = V(P)(s,k)`$ for the resolved term $`P`$.

The compute row takes the count of `static_authority_plan`. For each participant, that walk adds one count for every region link and one for every signed bind (`delta_sigma.rs:276-295`, `:489-497`). It also adds one count for each signed term whose body has no send or receive (`:426-435`). That extra count only raises the estimate, so the formula above is a lower bound of the compute row.

### 5.6 Exact sizes from representative values

The analysis computes every byte size from a representative value. It builds the value with the reducer's own functions and measures it with prost. [C][I]

1. **The sizing environment.** It has one entry for every level. Fresh and fresh-hole levels get a private name with a 32-byte identity. URN and injection levels get their values. Pattern levels get a poison marker.
2. **Send data.** The analysis runs the reducer's `eval_expr` on each datum, under the host-work budget. Then it substitutes and measures the result, as `eval_send` does (`reduce.rs:1467-1474`). Evaluation matters because operators such as the interpolation `%%` change the size of a closed datum (`reduce.rs:3215`, `:8173-8192`). If evaluation fails, the estimate is incomplete.
3. **Patterns and guards.** The analysis substitutes them at depth 1, as the reducer does (`reduce.rs:1581`, `:1596`).
4. **Receive bodies.** The analysis substitutes a receive body at depth 0 in the environment shifted by the bind count of the receive, as the reducer does (`reduce.rs:1632-1636`).
5. **The poison check.** A substituted introduced value that contains poison makes the estimate incomplete (rule X6). Each substitution uses its own environment. The bound levels of a receive stay unsubstituted variables inside its own body, so they are never poison there. Branching on the pattern variable of a receive therefore stays allowed.

The sizes are exact, because every sized field has a length that does not depend on run-time values.

- A private name has a 32-byte identity (`blake2b512_random.rs:169-178`).
- A region identity is a 32-byte Blake2b-256 hash (`authority.rs:404-424`).
- A serialized random state always has 536 bytes (`blake2b512_random.rs:248-286`, `blake2b512_block.rs:334-342`).
- Literal values appear in the term, so their sizes are known.
- A full environment makes each `locally_free` field equal to the one at run time.

One effect can only shrink a value: the runtime can merge two regions of a seal (Section 5.4). So each estimated size is equal to or larger than the size at run time.

### 5.7 The estimate in phlo

The raw counts carry no units. The analysis applies units once, in the known demand of each lane:

```math
\Delta^{\mathrm{known}}_s \;=\; \sum_{k} w_k \, u(s) \, V(s,k),
\qquad
B \;=\; \sum_{s} \Delta^{\mathrm{known}}_s.
```

The counted witness of the check takes the raw counts $`V(s,k)`$. Its weighted usage applies the weight and the units once (`phlo_execution.rs:493-503`), and the obligations multiply by the price (`obligations.rs:259-264`). So the usage of the witness equals $`B`$. A witness with units already applied would charge $`w_k u(s)^2 V(s,k)`$. Its usage would then exceed $`B`$ for every lane with more than one unit, and the check would reject such offers.

### 5.8 Cost of the analysis

The analysis is linear in the size of the resolved term. [I]

- **Pass 1** walks the term once. It assigns binder kinds, checks the rules, collects the participants with their region multisets, and sizes every participant. It also fills the table $`M`$.
- **Between the passes**, one loop over the receives fills $`T`$ and $`\mathrm{TR}`$. Its work is linear in the number of binds.
- **Pass 2** folds the term once into the vector $`V`$, with sums and maxima.

Each table is an array indexed by a binder identity, which pass 1 assigns when it enters a `new`. So each table access takes constant time. The evaluation and sizing work is bounded by the host-work budget. Both walks use explicit work stacks, so their stack depth does not grow with the nesting depth of the term (DR-67).

---

## 6. How the check uses the estimate

### 6.1 A complete estimate

A complete estimate holds exactly its cost. [D][I] The total hold is:

```math
H \;=\; B \cdot \mathrm{phloPrice} + 1.
```

The check reuses existing interfaces.

1. The raw counts $`V(s,k)`$ fill a `CountedPhloExecutionWitness` whose required and fresh resources are those counts.
2. `check_counted_phlo_execution` checks the witness against the bound $`B`$ (`phlo_execution.rs:517-577`).
3. `project_phlo_obligations` turns the witness into obligations for each lane and class, plus the fee (`obligations.rs:213`).
4. `select_phlo_funding_family` places the obligations on the eligible sources at $`R_0`$, with one requirement (`family_policy.rs:426`).

The meter then runs under the bound $`B`$ with the bound source `Certificate`.

### 6.2 An incomplete estimate

An incomplete estimate holds up to the signed limit. [D] Its total hold is:

```math
H \;=\; \mathrm{phloLimit} \cdot \mathrm{phloPrice} + 1,
\qquad
R \;=\; (\mathrm{phloLimit} - B) \cdot \mathrm{phloPrice}.
```

For an incomplete estimate, $`B`$ counts only the parts that the analysis can see. A part that it cannot see contributes 0, as DR-64 item 2 requires. Three kinds of part contribute 0.

- A participant whose value contains poison or fails to evaluate.
- The transfer and trace terms of a participant on a channel that is not fresh.
- The participants of a region whose signature is dynamic (DR-121).

The check places the holds in two steps.

1. It places the known demand for each lane and class on the eligible sources. It uses steps 1 to 4 of Section 6.1 under the bound $`\mathrm{phloLimit}`$. The fee goes on a fee-permitted source.
2. It fills the remainder $`R`$ over the sources in canonical custody order. Each source takes at most its balance, its hold cap and its debit cap, minus what step 1 placed on it.

The meter then runs under the bound $`\mathrm{phloLimit}`$ with the bound source `SignedLimit`. Supply counts SystemVault balances only. Prepaid phlo inventory does not count yet (an adopted gap fill of 2026-10-10).

### 6.3 Rejections before execution

The check rejects the deploy before any runtime entry in three cases. [P][C]

1. **The estimate exceeds the signed limit**, that is, $`B > \mathrm{phloLimit}`$. The existing bound check already rejects a bound above the limit (`phlo_bounds.rs:42-43`).
2. **The holds do not fit.** The selection finds no placement, a remainder is left, or the holds exceed the total exposure. The rejection text is "insufficient phlogiston" (P1:2210).
3. **The host-work budget runs out** in the resolver, the analysis or the selection (DRs:12012).

In each case, the deploy is rejected "without executing any part of the deployment. No state change occurs. No tokens are consumed." (P1:2210-2212).

### 6.4 How validators recompute the decision

Every validator recomputes the whole decision. [C][D]

- **Inputs.** The decision is a pure function of the five inputs of Section 5.1 and of the wallet snapshot at $`R_0`$.
- **Host work.** The check meters its work on its own host-work budget, with the consensus limits. Play and replay create that budget at the same point, just before evaluation. So the outcome does not depend on earlier work (DR-102, acceptance work on its own budget).
- **URN maps.** Play and replay build the URN map with the same constructor and the same inputs (`rho_runtime.rs:1997-1999`, `:2065-2068`). External services do not change the map.

Three checks detect a difference between the producer and a validator:

1. Validators re-run the check. A block whose offer fails the check is invalid.
2. The budget evidence stores the bound source of the contract (DR-113 item 5, DRs:11988-11991).
3. With several offers per block, validators recompute the guard outcomes (DR-65 item 6).

Replay also compares the encoded funding case byte for byte (`replay.rs:432-438`). That case now carries holds equal to debits.

---

## 7. Execution outcomes

The meter enforces the bound that the check chose. The table lists every outcome. [C][D]

| Outcome | Bound source | Class | Effect |
|---|---|---|---|
| Usage stays within the bound | Either | Normal | Settlement under the guard (Section 8) |
| Total usage passes $`B`$ | `Certificate` | Certificate failure | Reject, no charge. It means an analyzer defect, which DR-64 classifies as a correctness failure (DRs:3962-3964, DRs:12010). |
| One lane and class pass their estimate, but the total stays within $`B`$ | `Certificate` | No meter event | Settlement charges the use when eligible sources have balance. Otherwise settlement is infeasible, and the deploy is rejected with no charge. Both cases mean an analyzer defect. |
| Usage passes $`\mathrm{phloLimit}`$ | `SignedLimit` | Classified user failure | Roll back, charge the billable usage times the price plus 1, and publish as failed (DRs:12009) |
| A host-work limit | Either | Platform failure | Reject, no charge (DRs:12012) |

The billable usage is the fresh usage that the run drew before it failed (`phlo_execution.rs:559-564`).

---

## 8. Settlement and the guard

### 8.1 Settlement charges the measured use

Settlement keeps the rule of today: it charges the measured use directly. [C]

- After execution, the selection gets one requirement, the measured obligations. So each hold equals its debit (`family_selection.rs:201-218`, `funding_reservation.rs:65-72`).
- The holds of the check are fit checks at $`R_0`$. They move no funds (DR-5).
- SystemVault reserves each allocation from the balance at settlement. It fails with "Insufficient funds" when a purse is short (`SystemVault.rho:640-675`).
- The funding evidence stays valid with hold equal to debit and refund 0. It requires debit at most hold, fee at most debit, and refund equal to hold minus debit (`sections.rs:173-180`).

So refunds stay implicit, and offered state roots do not change. There is no settlement floor, and step G1-5 is dropped.

### 8.2 Why the floor is not needed

P1 needs three properties from settlement. Each one holds without a floor. [P][I]

1. **The refund.** P1 refunds $`\Delta_c^{\max}(D) - \kappa`$ (P1:2149-2151). A direct charge of the measured use leaves the same final balance as a hold of the maximum followed by a refund of the rest.
2. **The commitment.** Held resources stay unavailable to other deployments (P1:2301-2302). Across blocks, each block settles its offers before the next block funds from its own $`R_0`$. So no commitment spans two blocks. Sibling blocks depend on the merge rule, not on holds. Within a block, the guard of Section 8.3 enforces the commitment.
3. **The debit bound.** The meter stops the usage at the bound: $`B`$ under `Certificate`, and $`\mathrm{phloLimit}`$ under `SignedLimit`. The total debit is therefore at most the bound times the price, plus 1 (`native_phlo_rules/execution.rs:274-287`, `phlo_execution.rs:561-564`). Each source stays within its signed caps (`sections.rs:173-178`).

### 8.3 The guard

The guard is exactly item 5 of DR-65 as adopted (DRs:4032-4040). [D] Consider the settlement of member $`i`$ and a purse $`p`$.

- $`\mathrm{balance}_{\mathrm{after}}(p)`$ is the balance of $`p`$ just before the debit.
- $`\mathrm{debit}_i(p)`$ is the debit of member $`i`$ on $`p`$, with the fee included.
- $`\mathrm{hold}_j(p)`$ is the hold of a later member $`j`$ on $`p`$.

Every purse $`p`$ in the scope of the guard must satisfy:

```math
\mathrm{balance}_{\mathrm{after}}(p) - \mathrm{debit}_i(p) \;\geq\; \sum_{j > i} \mathrm{hold}_j(p).
```

- **Two conditions in one.** The guard implies balance at least debit, because the holds are not negative. That is the condition under which SystemVault can reserve the debit. The guard also keeps the later holds covered.
- **One offer per block.** The sum is 0, so the guard equals the condition of SystemVault.
- **Scope.** The guard covers every purse that member $`i`$ debits, installer purses included. It also covers every purse that carries a later hold. It reads every balance at the settlement root of member $`i`$.
- **Failure.** If the guard fails, member $`i`$ is a charged user failure. It rolls back to its execution root and pays its billable work and fee from the restored balance (DR-65 item 5).

**Example.** A purse has 1,000,000 atomic units at $`R_0`$. Member 1 holds 500,001 of them, and member 2 holds 390,001. The group check passes, because 890,002 is at most 1,000,000. During execution, the code of member 1 pays 600,000 out of the purse and leaves 400,000. Its measured charge is 20,000.

- SystemVault alone could still reserve the debit of 20,000. But then only 380,000 would remain for the hold of member 2.
- The guard of member 1 fails, because 400,000 minus 20,000 is less than 390,001. Member 1 becomes a charged user failure, and member 2 keeps its hold.
- Without member 2, the guard of member 1 passes, and settlement charges 20,000.

An earlier amendment, decision B3, also required the balance after execution to cover the member's own hold. That condition existed only because of the floor. With hold equal to debit, DR-65 item 5 is exact, and B3 is no longer needed.

### 8.4 Several offers per block

Several offers per block (G2) need one more rule, which replaces the floor. [I] Each member's funding capacity on each purse is capped, on the success path and on the rollback path. Let $`\mathrm{balance}(p)`$ be the balance of $`p`$ at the settlement root on the success path, and at $`R_{i-1}`$ on the rollback path. The cap of member $`i`$ on purse $`p`$ is:

```math
\mathrm{cap}_i(p) \;=\; \mathrm{balance}(p) - \sum_{j > i} \mathrm{hold}_j(p).
```

Each source on $`p`$ also stays within its signed hold cap and debit cap.

Without the cap, the selection could draw a purse that a later member holds. A rollback charge could also consume later holds. With the cap, induction keeps every later hold covered:

1. **Base.** Let $`\mathrm{supply}_{R_0}(p)`$ be the balance of $`p`$ at $`R_0`$. The group check of DR-65 gives $`\mathrm{supply}_{R_0}(p) \geq \sum_{m} \mathrm{hold}_m(p)`$, where $`m`$ ranges over the members of the group.
2. **Step.** Assume that the balance at $`R_{i-1}`$ is at least $`\sum_{j \geq i} \mathrm{hold}_j(p)`$. Member $`i`$ debits at most $`\mathrm{cap}_i(p)`$ on either path. So the balance after member $`i`$ is at least $`\sum_{j > i} \mathrm{hold}_j(p)`$.
3. **Rollback fits.** On the rollback path, the capacity is at least $`\mathrm{hold}_i(p)`$. For a complete estimate, the placement of the check then dominates the rollback charge.

A single offer per block (G1) needs no change, because it has no later members.

### 8.5 Installer purses

No hold of the check covers a debit on an installer purse under G3. [D] If an installer purse is short, the approved interim rule B5 applies: the node rejects the triggering deploy with no charge. A later tuple-space veto can replace B5.

### 8.6 Amended wording of DR-64

This design amends the wording of DR-64 items 4 and 6 and of its verification obligation. [D]

- **Item 4.** A hold is the fit check at $`R_0`$. A complete estimate holds its cost plus the fee. An incomplete estimate holds the known demand per lane and class, plus the complete remainder.
- **Item 6.** SystemVault reserves the measured debit. The refund is implicit: the unused part never leaves the purse.
- **Obligation.** "A dynamic offer holds up to the limit and gets the rest back" now means that the rest never leaves the purse.

---

## 9. The algorithm in literate form

This section presents the check as one top-level chunk built from named chunks, in the literate style of Knuth (1984). The text before each chunk explains it. A name in angle brackets refers to the chunk that defines it.

```text
⟨pre-execution check of one offered deploy⟩ ≡
  ⟨read the inputs⟩
  ⟨resolve the names of the deploy⟩
  ⟨run the linear analysis⟩
  ⟨reject an estimate above the signed limit⟩
  ⟨place the holds or reject⟩
  ⟨execute under the bound⟩
  ⟨settle under the guard⟩
```

The inputs are pure values. The producer and every validator read the same values from the envelope and from consensus state.

```text
⟨read the inputs⟩ ≡
  term, env ← envelope.body.term, normalizer_env_from_envelope(envelope)
  seed      ← user_envelope_rng(envelope)                      -- tools.rs:20-27
  funding   ← funding_sig(signers of envelope)                 -- runtime.rs:165
  w         ← weights of the adopted schedule
  urn_map   ← offered URN map                                  -- rho_runtime.rs:1997-1999
  host      ← new host-work budget with the consensus limits
  snapshot  ← wallet snapshot at R0: balances, caps, cursors
```

The resolver binds the names that the reducer allocates at resolving positions. It leaves holes inside receive bodies (DR-121).

```text
⟨resolve the names of the deploy⟩ ≡
  P ← resolve_lexical_names_for_funding_metered(normalize(term, env), seed, urn_map, host)
  if the host-work budget ran out: reject(envelope), no charge
```

The analysis has five named steps. Pass 1 covers the first two steps, and pass 2 covers the fourth.

```text
⟨run the linear analysis⟩ ≡
  ⟨assign binder kinds and check the rules⟩
  ⟨size every participant⟩
  ⟨build the channel tables⟩
  ⟨fold the demand vector⟩
  ⟨compute the estimate⟩
```

The kind stack mirrors the binders that the reducer creates. The first failed rule becomes the reason of an incomplete estimate.

```text
⟨assign binder kinds and check the rules⟩ ≡
  complete ← static_authority_plan(P, funding) is Ok                        -- X1
  complete ← complete and no signed source is an on-chain payer
  walk P with an explicit work stack and a kind stack:
    new with simple names and URIs:
      push fresh for each simple name, or fresh hole inside a receive body
      push URN or injection for each URI, at every depth
    receive or match case:
      push one pattern level for each bound variable
    at each execution position:
      each send or receive channel is a level of kind fresh or fresh hole   -- X2
      no method call among the top-level expressions                        -- X8
      no peek bind and no cost stack                                        -- X7
    each fresh or fresh-hole level occurs only as such a channel
      or in a cost signature                                                -- X3
    send data are closed                                                    -- X4
    receive patterns use no outer level                                     -- X5
    pattern levels occur only in if conditions and match targets            -- X6
  record the first failed rule as the reason
```

Each participant gets its exact size from a representative value. Poison marks a value that depends on received data.

```text
⟨size every participant⟩ ≡
  env ← one entry per level: a 32-byte private name for fresh and fresh-hole levels,
        the URN map or injection value for URN and injection levels,
        poison for pattern levels
  for each send o:
    data ← eval_expr(d, env) for each datum d, under host        -- reduce.rs:1467-1474
    if evaluation fails: complete ← false, the sizes of o count 0
    msg(o) ← datum message of substitute(data, depth 0, env),
             a 536-byte random state and the seal Λ_o
    ib(o)  ← |ch(o)| + |msg(o)| + 64
  for each receive r:
    cont(r) ← continuation of substitute(body(r), depth 0, env shifted by bind count),
              the guard at depth 1 and the seal Λ^C_r
    ib(r)   ← Σ over binds b of (|ch(b)| + |pat(b)| at depth 1) + |cont(r)| + 32 + 32·n_r
  if a sized value contains poison: complete ← false (X6), and that size counts 0
```

The channel tables need every datum size first, so they come after pass 1. A repeated channel in a join counts twice.

```text
⟨build the channel tables⟩ ≡
  for each send o:      M[ch(o)] ← max(M[ch(o)], |msg(o)|)
  for each receive r:   t ← Σ over binds b of M[ch(b)]
                        for each bind b of r:
                          T[ch(b)]  ← max(T[ch(b)], t)
                          TR[ch(b)] ← max(TR[ch(b)], 32 + 96·n_r)
```

Pass 2 folds the term into the raw-count vector. Branch points take maxima, so the vector dominates every branch.

```text
⟨fold the demand vector⟩ ≡
  V ← fold over P with an explicit work stack:
        parallel parts  → sum of their vectors
        if, match       → componentwise maximum of the branch vectors
        send o          → v_o, from Λ_o, ib(o), T and TR
        receive r       → v_r + vector of body(r), from Λ^I_r, Λ^C_r, ib(r), M
```

The known demand applies the weights and the units once. The raw counts stay unweighted for the witness.

```text
⟨compute the estimate⟩ ≡
  for each lane s: known[s] ← Σ over classes k of w[k] · u(s) · V[s][k]
  B ← Σ over lanes s of known[s]
```

The signed limit caps what the client authorizes. An estimate above it cannot be held.

```text
⟨reject an estimate above the signed limit⟩ ≡
  if B > phloLimit: reject(envelope), no charge                 -- phlo_bounds.rs:42-43
```

The placement differs only in the remainder. A complete estimate has no remainder.

```text
⟨place the holds or reject⟩ ≡
  known_holds ← select_phlo_funding_family(obligations(V) · phloPrice and the fee, snapshot)
  if complete:
    holds, bound ← known_holds, (B, Certificate)
  else:
    R ← (phloLimit − B) · phloPrice
    holds ← known_holds plus R filled in custody order,
            each source up to min(balance, hold cap, debit cap) minus known_holds
    bound ← (phloLimit, SignedLimit)
  if no placement, R is not fully placed, or Σ holds > total exposure:
    reject(envelope, "insufficient phlogiston"), no charge
```

Execution runs in a fresh offered runtime. The outcome decides the next step.

```text
⟨execute under the bound⟩ ≡
  result ← execute(P, bound)
  if result is within the bound:          ⟨settle under the guard⟩
  if the signed limit is exhausted:       user failure, roll back, charge billable · phloPrice + 1
  if the certified bound is exhausted:    reject, no charge, log a correctness failure
  if a host-work limit is reached:        reject, no charge
```

Settlement charges the measured use. The guard protects the later holds of the block.

```text
⟨settle under the guard⟩ ≡
  capacity(p) ← min(signed caps, balance(p) − Σ_{j>i} hold_j(p))     -- 0 later holds in G1
  debits ← select_phlo_funding_family(measured obligations, capacity)
  for every purse p that member i debits or that carries a later hold:
    guard(p) ← balance_after(p) − debit_i(p) ≥ Σ_{j>i} hold_j(p)     -- DR-65 item 5
  if every guard(p) holds:
    settle(debits), with hold = debit and refund = 0
  else:
    roll back to the execution root
    charge billable work and fee from the restored balance, under the same capacity
```

---

## 10. Worked examples

### 10.1 The term of the 64-source test

The test `offered_funding_uses_all_64_sources_with_one_fee_across_validators` signs this term (`offered_source_cap_api_test.rs:37`, `:207`):

```rholang
{% new x in { x!(0) | for (@n <- x) { Nil } } %}[ payer ]
```

- **Kinds.** The name `x` is fresh. The variable `n` is a receive-pattern level, and the body does not use it.
- **Rules.** X1 holds, because the plan returns `Ok`. X2 and X3 hold, because `x` is fresh and appears only as a channel. X4 to X8 hold. The estimate is complete.
- **Regions.** The signature `payer` is an unbound name, so it normalizes to a Ground signature with 1 unit (`sig.rs:43-48`). Its scope gives one region. The send and the receive both carry that region.

Let $`d`$ be the length of the datum message of `x!(0)`. Then $`M(x) = T(x) = d`$ and $`\mathrm{TR}(x) = 128`$. The table compares the estimate with the realized charge of lane `payer`.

| Class | Estimate | Realized |
|---|---|---|
| Compute | 1 + 1 = 2 | 1 |
| Introduction | $`\mathrm{ib}(o) + \mathrm{ib}(r)`$ | $`\mathrm{ib}(o) + \mathrm{ib}(r)`$ |
| Transfer | $`T(x) + M(x) = 2d`$ | $`d`$ |
| Trace | 128 + 128 = 256 | 128 |

The realized COMM charges the shared region once, but the estimate counts it once for each participant. The test schedule sets every weight to 1 and the price to 2 (`offered_source_cap_api_test.rs:51`, `:53`). So the deploy holds $`H = 2B + 1`$, which every source can hold within its caps.

### 10.2 A branch on a received value

```rholang
new x, y in {
  x!(5) |
  for (@n <- x) {
    if (n > 3) { y!(1) | for (_ <- y) { Nil } } else { Nil }
  }
}
```

- The deploy has no signed term. So each top-level send and receive opens its own region of the deploy lane.
- The body of the receive runs with an empty authority. So `y!(1)` and its receive also open their own regions of the deploy lane.
- The variable `n` occurs only in the `if` condition, so rule X6 holds. The estimate is complete.

Let $`o_1`$ be `x!(5)`, $`r_1`$ the receive on `x`, $`o_2`$ the send `y!(1)` and $`r_2`$ the receive on `y`. The deploy lane gets $`V(P) = v_{o_1} + v_{r_1} + (v_{o_2} + v_{r_2}) \sqcup 0`$, where 0 is the vector of the empty branch. For compute, this is 1 + 1 + 2 = 4. The datum 5 makes the true branch run, so two COMMs run. Each COMM carries two regions, one from each participant. So the realized compute is also 4. With the datum 2, only one COMM runs, and the realized compute is 2. Every participant here has its own region, so the gap comes only from the branch that does not run.

### 10.3 A join with two signed binds of one signature

```rholang
new x, y in {
  x!(0) | y!(0) |
  for ({% @a <- x %}[ s ] & {% @b <- y %}[ s ]) { Nil }
}
```

- Each send opens its own region of the deploy lane.
- The receive introduces itself under one region of the deploy lane, because the enclosing authority is empty.
- Its COMM seal holds two regions of lane `s`, one for each signed bind, and no deploy region.

The COMM charges all four regions. Lane `s` realizes transfer $`2\,(M(x) + M(y))`$ and trace $`2 \cdot 224 = 448`$. The estimate of lane `s` gives the same values, because $`\mu_s(\Lambda^{\mathrm{C}}_r) = 2`$. A count by lane membership would give $`M(x) + M(y)`$ and 224, which is half of the realized charge.

### 10.4 Sizes after evaluation

```rholang
new x in { x!("${a}${a}${a}${a}" %% {"a": "abcdefghij"}) | for (@n <- x) { Nil } }
```

The datum is closed, so rule X4 holds. The reducer evaluates the interpolation before it stores the datum, so the stored string has 40 characters. The analysis evaluates the datum in the same way and sizes the 40-character string. A size of the unevaluated term would undercount the introduction and the transfer.

### 10.5 Incomplete estimates

Each term below makes the estimate incomplete. The offer then holds its whole signed limit.

A send on a public name fails X2. The name can reach a stored contract or stored data.

```rholang
{% @"pub"!(0) %}[ payer ]
```

A method call in process position fails X8. The reducer runs the list element as a process.

```rholang
{% new x in { x!(0) | for (@n <- x) { Nil } | [ @"pub"!(0) ].nth(0) } %}[ payer ]
```

A URN inside a receive body fails X2, because `out` keeps its URN kind there. The send calls a system process whose work the estimate cannot see.

```rholang
new x in { x!(0) | for (@n <- x) { new out(`rho:io:stdout`) in { out!(0) } } }
```

A persistent receive fails X1, because it can recurse.

```rholang
new x in { contract x(@n) = { x!(n) } | x!(0) }
```

A received value in data fails X4 and X6. The size of the datum of `y!(n)` depends on the run.

```rholang
new x, y in { x!(0) | for (@n <- x) { y!(n) } | for (_ <- y) { Nil } }
```

A peek fails X7. A peeked datum can be delivered more than once.

```rholang
new x in { x!(0) | for (@n <<- x) { Nil } }
```

---

## 11. Soundness argument

This section argues that a complete estimate bounds the realized charge. The planned module `ClosedDemandBound.v` mechanizes the same argument in Rocq, the proof assistant formerly named Coq (Section 12). [I]

**Theorem (sound estimate).** Assume that the estimate is complete. Take any execution of the deploy, from any pre-state and under any order of matching. For every lane $`s`$ and class $`k`$, its realized charge is at most $`V(s,k)`$. So its realized usage is at most $`B`$.

The argument uses two hypotheses. The Rocq module states them as section hypotheses with witnesses, as `ReceiveBodyNames.v` does (DR-121).

- **Collision freedom.** Allocation never gives two different random states the same name.
- **Unique deploys.** Two deploys never share an envelope identity, so they never share a seed.

**Lemma 1 (each participant runs at most once).** Every process that runs is a subterm at an execution position, and each such subterm runs at most once.

- The table lists every site where the reducer runs a process. `owned_evaluation_terms` selects the two expression sites (`util/mod.rs:136-145`).

  | Site | Code | Status under the rules |
  |---|---|---|
  | Entry points | `reduce.rs:322`, `:348`, `:543` | Execution position |
  | Receive bodies | `reduce.rs:382-406`, `dispatch.rs:79` | Execution position |
  | Match cases | `reduce.rs:1763` | Execution position |
  | `if` branches | `reduce.rs:1823`, `:1835` | Execution position |
  | `new` bodies | `reduce.rs:1872` | Execution position |
  | Bundles | `reduce.rs:1911` | Execution position |
  | Signed terms | `reduce.rs:1996` | Execution position |
  | Result of an `EVarBody` | `reduce.rs:1377` | Excluded by rule X1 |
  | Result of an `EMethodBody` | `reduce.rs:1388` | Excluded by rule X8 |

- The top-level term runs once. A body of a signed term, a `new` or a bundle runs once each time its parent runs. A match or an `if` runs at most one branch.
- Rule X1 excludes persistent receives. So each receive fires at most once, and its body runs at most once.
- By induction on the nesting depth, each subterm runs at most once. So each participant is introduced at most once, and each `new` allocates its names at most once.

**Lemma 2 (no foreign participant).** Every COMM that involves a participant of the deploy has only participants of the deploy.

- By rule X2, every channel of a participant is a fresh name of the deploy. By rule X3, such a name never appears in data, patterns or expressions. So no outside process receives it.
- The names derive from the seed of the deploy. By the two hypotheses, a fresh name differs from every name in the pre-state.
- RSpace matches at introduction, so the pre-state holds no pair that can match. Every COMM during execution therefore involves a participant that the deploy introduces.
- No system process runs, because rule X2 excludes URN channels and rule X8 excludes method results.

**Lemma 3 (at most one COMM per participant).** Each introduced datum and each introduced continuation joins at most one COMM. Rule X1 makes every send and receive non-persistent, and rule X7 excludes peeks. A COMM removes the datum and the continuation that it consumes.

**Lemma 4 (sizes).** Assume rules X4, X5 and X6 and the sizing rules of Section 5.6. Then every introduced value is at most its estimated size at run time. The sized fields have value-independent lengths, and the data are evaluated as the reducer evaluates them. Merged regions only shrink seals.

**Lemma 5 (charge of one COMM).** Let $`\gamma`$ be a COMM of receive $`r`$ with the matched sends $`o_1, \ldots, o_{n_r}`$. Fix a lane $`s`$ and one of the classes compute, transfer and trace. Then the charge of $`\gamma`$ is at most $`v_r(s,k) + \sum_{i} v_{o_i}(s,k)`$.

- The authority $`U(\gamma)`$ of the COMM is the merged set of the seals of its participants (DRs:8311-8312). So each region of $`U(\gamma)`$ appears in $`\Lambda^{\mathrm{C}}_r`$ or in some $`\Lambda_{o_i}`$. Assign each region to one participant that carries it.
- **Channels.** Each send $`o_i`$ matched a bind $`b_i`$ of $`r`$ on one fresh name. By collision freedom and Lemma 1, one binder gives that name, so $`\mathrm{ch}(b_i) = \mathrm{ch}(o_i)`$.
- **Compute.** Each region charges 1, and the participant that carries it counts at least 1.
- **Transfer.** Each region charges $`\sum_{i} \lvert \mathrm{msg}(o_i) \rvert`$. By Lemma 4 and the definition of $`M`$, $`\lvert \mathrm{msg}(o_i) \rvert \leq M(\mathrm{ch}(b_i))`$. So the charge is at most $`\sum_{b} M(\mathrm{ch}(b))`$, which a region of $`r`$ counts. Since $`r`$ has a bind on $`\mathrm{ch}(o_i)`$, that sum is also at most $`T(\mathrm{ch}(o_i))`$. A region of $`o_i`$ counts that value.
- **Trace.** Each region charges $`32 + 96\,n_r`$. A region of $`r`$ counts exactly that. A region of $`o_i`$ counts $`\mathrm{TR}(\mathrm{ch}(o_i))`$, which is at least that.

**Lemma 6 (branches).** The componentwise maximum dominates the vector of every branch, and the channel tables range over the participants of all branches. A participant inside a receive body runs only when the receive fires. By induction on the structure of the term, $`V(s,k)`$ is at least the sum of the contributions of the participants that run.

**Proof of the theorem.** By Lemma 2, only COMMs and introductions of the participants of the deploy charge its regions. Fix a lane $`s`$ and a class $`k`$ among compute, transfer and trace.

1. By Lemma 5, the charge of each COMM is at most the sum of the contributions of its participants.
2. By Lemma 3, each participant joins at most one COMM. So the total COMM charge is at most the sum of the contributions of the participants that run.
3. By Lemma 1 and Lemma 6, that sum is at most $`V(s,k)`$.

For the introduction class, each introduction charges $`\mathrm{ib}`$ once for each region of its introduction authority. Lemma 4 bounds each size, and Lemma 6 bounds the sum by $`V(s,\mathrm{intro})`$. All terms are non-negative. Multiplying by $`w_k u(s)`$ and summing gives a realized usage of at most $`B`$. ∎

The argument quantifies over every order of matching, and COMM matching does interleave. This quantification is the reason why the estimate needs no separate model. Such a model would use TLA+, the specification language of the temporal logic of actions.

---

## 12. Verification plan

The verification follows the bar of this branch: formal models with negative controls, one property test for each invariant, tests and documentation. Every Rocq module is axiom-free and ends with `Print Assumptions`. [I]

**Rocq modules (planned).**

- `ClosedDemandBound.v` proves the theorem of Section 11 as `complete_estimate_sound`. It quantifies over every trace of the nondeterministic matching relation. It models authorities as region multisets. Its lemmas are `closed_comm_bound`, `closed_intro_bound`, `closed_transfer_bound`, `closed_trace_bound`, `branch_max_dominates`, `fresh_unescaped_no_foreign_match` and `units_applied_once`.
- `EstimateDemandWalk.v` instantiates the generic walk section of `StackSafeDemandWalk.v` (`:409-424`). It covers the vector pass only, because the walk term has no binders.
- `EstimateBinderKinds.v` models the binder analysis over de Bruijn terms with the six kinds. Its lemmas are `kinds_channels_fresh`, `kinds_no_escape`, `kinds_poison_free_introductions` and `full_environment_matches_runtime_free_levels`.
- `BoundKindSoundness.v` proves that the check never treats an incomplete estimate as complete, and that a rollback after an overrun has no effect.
- The guard lemmas of DR-65 are `group_holds_prevent_overdraft`, `held_capacity_guard_preserves_later_holds` and `group_decision_permutation_invariant`. They are restated for settlement without a floor, with the capacity cap of Section 8.4 on both paths.

**Negative controls.** Each control is a deliberately broken variant that a witness must refute.

- A public channel, an escaping name, a persistent receive and a peek.
- A received value in data, and a received value inside a nested introduction.
- The minimum instead of the maximum over branches.
- A method call in process position.
- An introduction charged to the regions of signed binds.
- Units applied twice: the usage $`w_k u(s)^2 V(s,k)`$ then exceeds $`B`$ when $`u(s) = 2`$.
- A join with a repeated channel counted as a set.
- Lane membership instead of region multiplicity.
- A datum sized before evaluation.
- A URI level treated as fresh inside a receive body.
- A poison check that flags the bound levels of a receive inside its own body.
- A rollback charge without the capacity cap. It must break `held_capacity_guard_preserves_later_holds`.

**Rust tests.**

- One generated program for each rule X1 to X8 and for the on-chain payer condition. None of them may get a complete estimate.
- Named programs, each with its own test:
  - the method-call program and the URN program of Section 10.5
  - the join of Section 10.3, and the interpolation of Section 10.4
  - a nested signed scope in a term with several parallel parts
  - `new x in { x!(0) | for ({% @n <- x %}[ s ]) { Nil } }`, whose receive has a signed bind
- Parity of the iterative walk with a recursive oracle on 256 seeds, and a term nested 100,000 deep on the default stack (DR-67).
- A runtime differential test: measured at most estimated for every lane and class. It runs on generated programs with complete estimates and on the named programs. The named set includes the term of Section 10.1, a two-signer offer, a join with a repeated channel and a branch on a received value.
- The level counts of the binder machine equal those of the resolver on the Rholang corpus.
- Play and replay reach the same decision. The outcome of the check does not depend on earlier budget use. The URN maps of play and replay are equal.
- Settlement evidence has hold equal to debit and refund 0. Balances satisfy post-state balance equal to pre-state balance minus debit.
- A self-drain becomes a charged user failure, and the receipts of producer and validators agree.
- A property test of feasibility. Assume that the realized use is at most the estimate for every lane and class, and that balances do not change. Then the post-execution selection succeeds. The capacity is the balance in G1, and the balance minus later holds in G2.
- The 64-source test passes under the committed caps.

---

## 13. What the estimate cannot see

The analysis sees only the deploy's own text. These constructs make the estimate incomplete, and the margin rule of P1 covers them. [C]

- Work inside stored contracts that the deploy triggers.
- Data received from stored state.
- Persistent or recursive processes.
- The dequotation `*x`.
- Method calls in process position.
- Peeks.

The rules also make the estimate incomplete for these constructs.

- Cost stacks.
- Public, URN and injected channels, at every depth.
- Signatures that name a received value or a name created in a receive body (DR-121).
- Data whose evaluation fails.
- Sources that are on-chain private-name payers.

Most real deploys call stored contracts, such as a registry lookup or a vault transfer. So most real deploys get an incomplete estimate and hold their whole signed limit. [I]

**A point for the discussion with Greg and the Casper team.** In this implementation, a triggered stored contract charges its unsigned work to the triggering deploy. The body runs with an empty authority, and each unsigned send or receive in it opens a region of the current payer (`reduce.rs:382-406`, DRs:8298-8302). P1 instead funds such a body from its own signature. Its uniform signing gives the continuation its own signed layer (P1:1251-1271), and "Neither party can impose costs on the other beyond what the sugar makes explicit." (P1:1313-1314). This difference affects communications and bytes alike, so it does not change the design of the estimate.

---

## 14. The client sizing rule

The estimate can exceed the realized use, so clients must size their limits to it. [I]

- **The gap.** The estimate counts each COMM once for each participant. When participants share a region, the estimate can reach twice the realized use in compute, transfer and trace. For the term of Section 10.1, the realized values 1, $`d`$ and 128 face estimates of 2, $`2d`$ and 256.
- **Wider gaps.** Branches that do not run and introductions that never match widen the gap further.
- **Rejection.** An offer whose estimate exceeds its signed limit is rejected before execution (P1:2208-2212, `phlo_bounds.rs:42-43`).
- **The rule.** Clients must sign a phlo limit at least as large as the estimate. The estimate is a pure function of the envelope and of public consensus inputs, so a client can compute it before signing.

**Residual risk of incomplete estimates with several sources.** The remainder fill ignores resource classes. So such an offer can still prove infeasible after execution, when the realized use lands on keys whose eligible sources lack capacity. The deploy is then rejected with no charge, and the producer's work is lost. An *obligation key* $`\theta`$ is one pair of a lane and a class, or the fee. Its *eligible sources* $`E(\theta)`$ are the sources that may pay key $`\theta`$. The capacity $`\mathrm{cap}(f)`$ of a source $`f`$ is the smallest of its balance, its hold cap and its debit cap. The client rule that removes this risk is:

```math
\sum_{f \in E(\theta)} \mathrm{cap}(f) \;\geq\; \mathrm{phloLimit} \cdot \mathrm{phloPrice} + 1
\qquad \text{for every obligation key } \theta.
```

An offer with one source that may pay every key, the fee included, meets the rule when that source can hold $`H`$.

**Why the rule suffices.** Let $`\delta_\theta`$ be the realized demand of key $`\theta`$, in atomic units. By the supply-demand theorem of Gale (1957), a feasible placement exists if and only if every set $`\Theta`$ of keys satisfies:

```math
\sum_{\theta \in \Theta} \delta_\theta \;\leq\; \sum_{f \in E(\Theta)} \mathrm{cap}(f),
\qquad
E(\Theta) \;=\; \bigcup_{\theta \in \Theta} E(\theta).
```

The meter keeps the total of all $`\delta_\theta`$ at or below $`\mathrm{phloLimit} \cdot \mathrm{phloPrice} + 1`$. For a nonempty set $`\Theta`$, choose any key $`\theta_0`$ in it. The left side is at most that total. By the client rule, the total is at most the capacity of $`E(\theta_0)`$, which is at most the right side. The empty set satisfies the condition trivially. The total exposure does not bind, because the check already placed the whole hold within it. The selection must also find the placement within its host-work budget, which the property test of Section 12 covers.

---

## 15. Relation to other documents

- **DR-64.** This design implements DR-64. A complete estimate holds its exact demand, as the verification obligation of DR-64 states. Section 8.6 amends the wording of items 4 and 6.
- **DR-65.** The guard is item 5 of DR-65, unchanged. The capacity cap of Section 8.4 implements the commitment of DR-65 when several offers share a block. It does not change the guard.
- **DR-113.** A complete estimate binds `Certificate`. An incomplete estimate binds `SignedLimit`, as every live contract does today.
- **The proposal, §4.1.** The proposal states the incomplete case and the remainder $`R`$. This document adds the complete case. It replaces the hold floor of the earlier design with the guard of Section 8.3.
- **[Byte accounting](vault-backed-byte-accounting.md).** That document defines the byte tariff. This document estimates the same tariff before execution.
- **[Pre-execution acceptance diagram](../diagrams/offered-preexecution-acceptance.puml).** That diagram of DR-64 shows the incomplete case. The diagram of Section 3 shows both cases.

---

## 16. Decision history

The user made these decisions on 2026-10-10. Times are in coordinated universal time (UTC). The quotations are the user's words or the user's selected answers, verbatim.

| Time | Decision |
|---|---|
| 15:13 | **B1, the first narrowing.** The user approved decisions B1 to B5 ("Approve all five"). B1 narrowed DR-64: every offer holds its full signed limit for now, and exact holds wait. B3 added the own-hold condition to the guard. B5 rejects a triggering deploy with no charge when an installer purse cannot pay. |
| 15:13 | **C1 and C2.** C1 raised the funded test vaults to $`10^{12}`$ atomic units ("Raise test vaults to 10^12 (Recommended)"). For the 64-source test, C2 chose exact holds ("Build an exact class"). |
| 16:17 | **One analysis, not a class.** The user asked: "Why is there a need for a specialized class for this? Isn't the linear solver generalized to cover these cases? If the full cost can be proven up front (no dynamicity in logic that would accrue costs), then that will be covered by the linear solver, right?" |
| 16:25 | **No separate class.** "Then let's scratch support for exact-class programs from the docket." |
| 16:26 | **Bytes in the linear solver.** "If you can linearly solve byte estimates up front like you can with COMMs, then include those estimates in the linear solver!" |
| 16:34 | **Occam's razor, bounded by the publications and Greg's expectations.** "Occam's Razor must still satisfy the publication documents about cost-accounting and Greg's expectations, that is why it only strips away what is unnecessary -- it does not mean oversimplify to the point of error in expectations." |
| 17:03 | **The adopted design.** The user answered "Approve (Recommended)" to this proposal. One linear analysis estimates communications plus bytes. A complete estimate holds exactly its cost, as P1 and DR-64 require. An incomplete estimate holds up to its signed limit, by the margin rule of P1. Settlement charges the measured use directly, with no floor, so step G1-5 is dropped. The guard covers the charge plus the fee, and the later holds within a block, so B3 is no longer needed. |

A second independent check of the design followed on the same day. Its corrections are part of this document.

- The byte classes count regions, not lane membership (Section 5.4).
- Send data are sized after evaluation (Section 5.6).
- URI levels keep their kind at every depth (Section 5.2).
- The poison check uses the own environment of each substitution (Section 5.6).
- Several offers per block cap the funding capacity on both paths (Section 8.4).
- Only a total overrun of a complete estimate is a certificate failure. Drift detection rests on three checks (Sections 6.4 and 7).
- The guard is DR-65 item 5 exactly, and its scope includes installer purses (Section 8.3).
- An installer shortfall follows B5 (Section 8.5).
- The client rule covers incomplete estimates with several sources (Section 14).

---

## 17. References

**Governing paper.**

- P1: L. G. Meredith, *Cost-Accounted Rho Calculus*, publications revision `0bf7817`, file `cost-accounting/cost-accounted-rho.tex`. Cited definitions: `def:token-demand` (P1:2002-2027), `thm:decidability` and its proof sketch (P1:2060-2080), `def:conservative-demand` (P1:2138-2152), the acceptance protocol (P1:2182-2213), uniform signing (P1:1251-1271) and the commitment (P1:2299-2304). [Source on GitHub](https://github.com/F1R3FLY-io/publications/blob/main/cost-accounting/cost-accounted-rho.tex).

**External works.** Each link resolves the digital object identifier (DOI) of the work.

- D. Gale, "A theorem on flows in networks", *Pacific Journal of Mathematics* 7(2), 1073-1082, 1957. [doi:10.2140/pjm.1957.7.1073](https://doi.org/10.2140/pjm.1957.7.1073).
- D. E. Knuth, "Literate Programming", *The Computer Journal* 27(2), 97-111, 1984. [doi:10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97).

**Repository documents.**

- [Decision records](../cost-accounting-decision-records.md): DR-5, DR-64, DR-65, DR-67, DR-68, DR-101, DR-102, DR-113, DR-118, DR-121.
- [The proposal](../cost-accounting-consensus-proposal.md), §1.2, §2.1, §3.6 and §4.1.
- [Vault-backed byte accounting](vault-backed-byte-accounting.md): the byte tariff.
- [Host-work budget](../host-work-budget.md): the budget of deterministic node work.

**Code at HEAD.**

- `rholang/src/rust/interpreter/accounting/delta_sigma.rs`: the analyzer that this design extends.
- `rholang/src/rust/interpreter/accounting/byte_accounting.rs`: the byte charge formulas.
- `rholang/src/rust/interpreter/accounting/lexical.rs`: the metered resolver.
- `rholang/src/rust/interpreter/reduce.rs`: the region, seal and evaluation rules.
- `rholang/src/rust/interpreter/accounting/native_phlo_rules.rs` and `native_phlo_rules/execution.rs`: the classes and the meter.
- `rholang/src/rust/interpreter/accounting/phlo_execution.rs`, `phlo_execution/obligations.rs` and `phlo_execution/family_policy.rs`: the witness, the obligations and the funding selection.
- `casper/src/rust/util/rholang/costacc/direct_wallet_funding/execution/family_selection.rs`: settlement selection today.
- `models/src/rust/native_cost_evidence/sections.rs`: the funding evidence relations.
- `casper/src/main/resources/SystemVault.rho`: the vault reservation.
