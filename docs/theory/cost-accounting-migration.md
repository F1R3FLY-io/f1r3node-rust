# Migrating F1R3Node to Internalized Cost Accounting: A Design for Mechanized Cost Determinism via the Cost-Accounted Rho Calculus

**Version:** 1.0
**Date:** 2026-04-08
**Authors:** Dylon Edwards, with formal verification contributions from L. Gregory Meredith
**Status:** Implementation-aligned design document

**Scope of this revision:** this document aligns the repo-local
verification record with the staged `f1r3node-rust` implementation.
It does not modify the external paper.

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Glossary](#2-glossary)
3. [The Baseline Externalized Architecture](#3-the-baseline-externalized-architecture)
4. [The Cost-Accounted Rho Calculus Solution](#4-the-cost-accounted-rho-calculus-solution)
5. [Architectural Changes](#5-architectural-changes)
6. [Migration Strategy](#6-migration-strategy)
7. [Formal Correctness (Pointer to Verification Doc)](#7-formal-correctness-pointer-to-verification-doc)
8. [Consensus Implications](#8-consensus-implications)

**Appendices**

- Appendix A — Rocq Module Inventory and Rocq ↔ TLA+ Correspondence
- Appendix B — The Five Cost-Accounted Reduction Rules
- Appendix C — File Locations

9. [References](#9-references)

---

## 1. Executive Summary

At the design baseline, before the staged cost-accounted rho implementation in this repository, the F1R3Node Rholang interpreter used an **externalized** cost-accounting model: a `CostManager` tracked remaining phlogiston, and a `ChargingRSpace` wrapper intercepted every `produce` / `consume` to pre-charge storage cost, refund on COMM match, and apply a unified COMM cost. Those names refer to the retired pre-migration architecture described by the design and proposal history, not to live current source paths in this branch. The strategy was order-independent by construction — confirmed empirically by `cost_should_be_deterministic` (which re-runs each contract 20 times and asserts identical cost) at `rholang/tests/accounting/cost_accounting_spec.rs:323`, by `cost_should_be_repeatable_when_generated` (10,000 randomly-generated contracts) at line 344, and by `peek_with_parallel_produce_should_have_deterministic_replay_cost` (line 474), which specifically guards the peek-with-parallel-produce class that is sensitive to scheduling.

This document specifies the upgrade from that externalized model to an **internalized** model derived from the cost-accounted rho calculus [4]. The internalized model offers three concrete advantages over the legacy `ChargingRSpace` baseline: (a) cost determinism is established by a **mechanized formal proof** (`ca_cost_deterministic` in `formal/rocq/cost_accounted_rho/Confluence.v`) rather than by empirical test coverage; (b) the retired per-COMM **pre-charge / refund / unified-charge logic** is no longer required, simplifying the eval-loop hot path; (c) **composable metering primitives** (paper §6.2: rate-limited markets, joint-account spending, prepayment, forwarding) become first-class language constructs.

The principled construction: **internalize cost accounting into the rho calculus itself** using the cost-accounted rho calculus [4], with a recursive metering kernel as the implementation target. The compositional translation — defined by four mutually recursive functions `N⟦·⟧`, `T⟦·⟧`, `P⟦·⟧`, `S⟦·⟧` — remains the paper trace and supplies the signature-channel, token, Split/Join, and local gate proofs. The implementation target is the machine-checked relation `well_reflected`: every enabled source step is represented by a continuation-keyed `recursive_metered_gate`, and the gate lands in a recursively metered image of the source successor. The theorem `well_reflected_backward_reflection` proves that every pure-rho step from that target reflects to an actual `ca_step`. Across multiple deploys in a block, confluence of the cost-accounted calculus (proven as `ca_cost_deterministic` in `Confluence.v`) ensures that cross-deploy interleaving does not affect the total cost. Together, recursive metering and formal confluence make cost a deterministic function of the deploy that is **machine-checked**, not merely sampled. The billable unit is source-token consumption under Rules 1-5, not every pure-rho COMM introduced by the translation.

The baseline strategy is correct but empirical-only and embeds nontrivial refund logic; the internalized model upgrades it to a mechanized, refund-free, composable model — strictly stronger guarantees, simpler runtime.

The correctness of this approach rests on two independent formal verifications:

1. **Rocq mechanization** (`formal/rocq/cost_accounted_rho/`): 23 modules with zero admissions and zero axioms. The consensus-critical results (token conservation, cost determinism, step determinism, fuel-gate safety, strong normalization, full confluence, fuel-event multiset determinism, channel separation, fee settlement, slashing composition, typed mergeable-channel accounting, bounded-memory runtime-budget refinement, replay-payload trace equivalence, and use-case adequacy) are unconditional except for the explicit translation-side hash hypotheses described below. One abstract `hash_process : list bool → proc` encoding parameter plus three explicit section hypotheses (`hash_process_injective`, `hash_process_closed`, `hash_process_head_count_one`) scope only the translation-side theorems that reason about hash-derived signature channels. The replication appendix is also axiom-free: it proves Meredith's reflective encoding performs the expected one-step unfold and that every weak input/output barb of the replicated body propagates to both the primitive `PReplicate` wrapper and the reflective `bang_encoding` wrapper (`replication_encoding_forward_barb_sound`). The headline results are: (a) the token conservation theorem (`token_monotone_step`, `token_monotone_reachable`): no reduction step creates fuel, and every step consumes a strictly positive amount; (b) the cost determinism theorem (`ca_cost_deterministic` in `Confluence.v`): all terminal states reachable from a given initial system have the same token count, proven via strong normalization (`StrongNormalization.v`) and local confluence (`ca_local_confluence`) composed through Newman's lemma; (c) the step determinism theorem (`ca_step_deterministic` in `StepDeterminism.v`): in a system with at most one `SToken` node (a single deploy), `ca_step` is deterministic — there is exactly one possible successor, formally capturing the sequential ordering enforced by the token chain; (d) the channel separation theorem (`ChannelSeparation.v`): fuel-gate channels are structurally disjoint from application channels, ensuring that multi-channel consumes (joins) in user code cannot interfere with cost accounting; (e) the runtime-budget refinement theorems (`RuntimeBudgetRefinement.v`): a coalesced counter preserves consumed/remaining conservation, out-of-phlo boundary commitment, finalization-read trace commitments, and reset-time trace clearing; (f) the slashing composition theorem family (`SlashingComposition.v`): cost-invalid evidence may feed slashing without changing user cost, and slash system effects preserve user fuel and fee settlement; and (g) the typed mergeable-channel theorem family (`MergeableChannelAccounting.v`): `IntegerAdd` keeps additive diff/merge semantics, `BitmaskOr` records newly-set bits and replays by OR, non-numeric tagged payloads stay outside numeric merge accounting, and mergeable metadata does not mutate the user cost boundary.

2. **TLA+ model** (`formal/tlaplus/cost_accounted_rho/`): eight specifications with concrete model-checking instances, verified by TLC with zero errors across all reachable states and cross-checked through Apalache for the typed threat/search-frontier models. The core protocol/scheduling models (`CostAccountedRho.tla`, `CompoundProtocol.tla`, `FullProtocol.tla`, `EvalScheduling.tla`) check token conservation, cost determinism, fuel-gate safety, gate ordering, and liveness across all interleavings — from a minimal 3-process atomic system (79 states) up to a fully generalized 7-process system with shared channels, arbitrary nesting (depth 0/1/2), Split mediators, Join mediators, and recursive eval (12,960 states). The implementation/security/search models (`RuntimeBudgetReplay.tla`, `CostAccountingThreats.tla`, `CostAccountingSearchFrontier.tla`, `MergeableChannelAccounting.tla`) check bounded runtime-budget replay, threat-model, slash-authorization, search-frontier, and typed mergeable-channel invariants.

Both verifications are documented in detail in the verification
companion, [*Formal Verification of Cost-Accounted Rho Calculus*](cost-accounted-rho-verification.md)
(henceforth "verification doc"). The Rocq proofs live in the
verification doc's Sections 6–9 and the TLA+ model in Section 10.
This document (the migration doc) focuses on *how* the translation is
integrated into F1R3Node and relies on the verification doc's theorems
rather than reproducing them.

The security and thread-safety boundary is documented separately in
[*Cost-Accounted Rho Threat Model*](cost-accounting-threat-model.md).
That document follows the slashing threat-model structure: adversary
model, STRIDE matrix, attack tree, coverage matrix, failure modes, and
formal/Rust/TLA+ traceability for cost-accounting threats.

With internalized cost accounting, F1R3Node's eval loop migrates from sequential `JoinHandle` awaiting to a `FuturesUnordered` driver over branch tasks, with recursive metering on the user-deploy path: no `ChargingRSpace` wrapper, no per-COMM refund bookkeeping, no Phase 1 / Phase 2 dispatch barrier, and no global cost mutex. Branch tasks may still be spawned to preserve true parallelism and isolate deep recursive stacks, but their completion is drained through `FuturesUnordered` and errors are reported in stable source order. Each ready source redex can be polled concurrently; the metering kernel authorizes exactly the selected source step, records the source-token match, and re-enqueues the recursively metered continuation. Cost is therefore a deterministic count of billable source-token consumption events regardless of scheduling order. The result is a correct, concurrent, formally verified cost model that restores the maximum parallelism the rho calculus was built for while preserving deterministic result aggregation.

**No changes to the Rholang language are required.** The entire migration is invisible to Rholang developers: the cost-accounting translation is applied automatically by the compiler pipeline (after normalization, before reduction), and existing Rholang programs run unchanged with zero source modifications. The new types (`Sig`, `Token`, `SignedProcess`) are internal compiler intermediate representations, not Rholang surface syntax. A Rholang programmer writing `for(@x <- ch){ P } | ch!(data)` today will write exactly the same code after the migration; the compiler will silently wrap the deploy in a fuel gate based on the deployer's cryptographic key.

---

## 2. Glossary

Terms are defined here in the order they are needed. Every symbol, acronym, and key term used in this document appears below before its first use in the body.

| Term                                 | Definition                                                                                                                                                                                                      |
|--------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| **Rho calculus**                     | The reflective higher-order process calculus [1], the theoretical foundation of Rholang. Distinguished by its reflective nature: names are quoted processes (`@P`), and processes can dereference names (`*x`). |
| **Rholang**                          | The smart contract programming language of the F1R3FLY.io blockchain platform [3], based on the rho calculus.                                                                                                   |
| **Phlogiston**                       | The unit of computational cost (gas) in Rholang. Named after the 18th-century chemical theory to emphasize that it is consumed during computation.                                                              |
| **COMM event**                       | The fundamental computational step of the rho calculus: a receiver `for(y ← x){P}` and a sender `x!(Q)` on the same channel `x` fire, substituting `@Q` for the bound variable `y` in `P`.                      |
| **Tuple space**                      | The shared data store (RSpace) where sends and receives are stored until matching partners arrive. Implemented as an associative structure keyed by channel.                                                    |
| **RSpace**                           | The F1R3Node implementation of the tuple space, using LMDB as its backing store. Provides `produce` (store a send) and `consume` (store a receive) operations. In this repository, defined in `rspace++/`. |
| **Deploy**                           | A user-submitted Rholang program plus metadata (cost limit, timestamp, signature). The unit of work in the F1R3Node blockchain.                                                                                  |
| **Validator**                        | A node that proposes and validates blocks. All validators must agree on the cost of every deploy for consensus to hold.                                                                                         |
| **`CostManager`**                    | Retired pre-migration role for a shared mutable phlogiston counter and cost log. The staged implementation replaces the user-deploy consensus role with `RuntimeBudget` and `MeteredMachine`. |
| **`ChargingRSpace`**                 | Retired pre-migration role for a broad RSpace wrapper that pre-charged `produce`/`consume`, refunded on COMM match, and applied unified COMM cost. The current staged implementation does not contain a live `charging_rspace.rs` source file. |
| **`RuntimeBudget`**                  | Current staged runtime-budget implementation in `rholang/src/rust/interpreter/accounting/mod.rs`. It records bounded billable source-token events, maintains consumed/remaining counters, and computes replay-authenticated cost trace digests. |
| **`MeteredMachine`**                 | Current staged metering adapter in `rholang/src/rust/interpreter/metering.rs`. It routes billable, nonbillable, and system frames into `RuntimeBudget` without reintroducing a broad RSpace charging wrapper. |
| **`Cost`**                           | A Rust struct with an `i64` value and a `Cow<'static, str>` operation label. Represents a phlogiston amount.                                                                                                    |
| **`storage_cost_produce`**           | Function computing phlogiston for a produce: `storage_cost(channel) + storage_cost(data.pars)`.                                                                                                                 |
| **`storage_cost_consume`**           | Function computing phlogiston for a consume: `storage_cost(channels) + storage_cost(patterns) + storage_cost(body)`.                                                                                            |
| **`FuturesUnordered`**               | `futures::stream::FuturesUnordered`, a Rust combinator that polls futures in completion order rather than submission order, enabling true concurrent evaluation. The post-migration eval loop is built around `FuturesUnordered`. |
| **`tokio::spawn`**                   | `tokio::spawn`, the runtime function that moves a future onto a separate tokio task. The migration keeps spawning available for true parallelism and stack isolation, but replaces sequential `JoinHandle` awaiting with `FuturesUnordered` completion-order draining plus stable error aggregation. |
| **Signature** (`sig`)                | An algebraic representation of a digital signature used to authorize fuel consumption. Three constructors: `SUnit` (trivial), `SHash(bs)` (atomic, from byte string), `SAnd(s1, s2)` (compound).                |
| **Token** (`token`)                  | A right-nested stack of signature-gated fuel units. `TUnit` is the empty token (no fuel). `TGate(s, t)` is one unit of fuel guarded by signature `s`, with remaining balance `t`.                               |
| **System** (`system`)                | A top-level term of the cost-accounted calculus: `SSigned(P, s)` (process `P` under signature `s`), `SToken(t)` (free-standing token), or `SPar(S1, S2)` (parallel composition).                                |
| **`N⟦s⟧`**                           | Signature translation: maps a signature `s` to a name (channel) in the pure rho calculus.                                                                                                                       |
| **`T⟦t⟧`**                           | Token translation: maps a token `t` to a process (parallel outputs on signature channels).                                                                                                                      |
| **`P⟦P, s⟧`**                        | Signed process translation: wraps process `P` in a fuel-gated input on `N⟦s⟧`.                                                                                                                                  |
| **`S⟦sys⟧`**                         | System translation: maps a system to a pure rho calculus process, composing `N`, `T`, `P` compositionally.                                                                                                      |
| **Fuel gate**                        | The input prefix `for(t ← N⟦s⟧) (P ∣ *t)` introduced by `P⟦P, s⟧`. It blocks execution of `P` until a matching token arrives on channel `N⟦s⟧`.                                                                  |
| **Token conservation**               | The invariant that the total number of fuel tokens in a system never increases under reduction. Proven as `token_monotone_step` and `token_monotone_reachable` in Rocq.                                         |
| **Cost determinism**                 | The property that the total cost (tokens consumed) at termination is independent of scheduling order. Verified as `CostDeterminism` in TLA+.                                                                    |
| **Fuel-gate safety**                 | The property that a signed process cannot communicate without first consuming a matching token. Proven as `fuel_gate_stuck_isolated` in Rocq.                                                                   |
| **Split**                            | A mediator process that decomposes a compound-signature token into atomic tokens: `Split(s₁, s₂)` inputs on `N⟦s₁ & s₂⟧` and outputs on both `N⟦s₁⟧` and `N⟦s₂⟧`.                                               |
| **Join**                             | The inverse mediator: `Join(s₁, s₂)` inputs on `N⟦s₁⟧` and `N⟦s₂⟧` and outputs on `N⟦s₁ & s₂⟧`.                                                                                                                 |
| **`P ∣ Q`**                          | Parallel composition of processes `P` and `Q` in the pure rho calculus.                                                                                                                                         |
| **`S₁ ∥ S₂`**                        | Parallel composition of systems `S₁` and `S₂` in the cost-accounted calculus.                                                                                                                                   |
| **`S ⤳ S'`**                         | A single cost-accounted reduction step from system `S` to system `S'`.                                                                                                                                          |
| **`S ⤳* S'`**                        | Zero or more cost-accounted reduction steps (reflexive-transitive closure).                                                                                                                                     |
| **`P ⇝ Q`**                          | A single pure rho calculus reduction step from process `P` to process `Q`.                                                                                                                                      |
| **`P ⇝* Q`**                         | Zero or more pure rho calculus reduction steps.                                                                                                                                                                 |
| **`‖S‖`**                            | The total token count (fuel measure) of system `S`: `system_token_count(S)`.                                                                                                                                    |
| **`@P`**                             | Quotation: turns process `P` into a name (channel). Written `Quote P` in Rocq.                                                                                                                                  |
| **`*x`**                             | Dereference: turns name `x` back into a process. Written `PDeref x` in Rocq.                                                                                                                                    |
| **De Bruijn index**                  | A naming convention for bound variables using natural numbers: `NVar 0` is the most recently bound, `NVar 1` the next outer, etc. Eliminates alpha-equivalence issues.                                          |
| **Structural equivalence** (`P ≡ Q`) | The equivalence relation on processes generated by commutativity, associativity, and identity of parallel composition, plus congruence under all constructors.                                                  |
| **Lift** (`lift_proc d c P`)         | Shifts de Bruijn indices ≥ `c` in process `P` by `d`, used when placing a term under additional binders.                                                                                                        |
| **TLC**                              | The TLA+ model checker, which exhaustively enumerates reachable states.                                                                                                                                         |
| **Rocq**                             | The formal proof assistant formerly known as Coq (renamed in version 9.0). Used for the mechanized proofs in `formal/rocq/cost_accounted_rho/`.                                                                 |
| **Stuck residue**                    | The inert term `*(@0)` (i.e., `PDeref(Quote(PNil))`) produced by a fuel-gate COMM firing. Structurally equivalent to `PNil` by the quote-dereference cancellation law. Provably harmless via the bisimulation proof (`multi_stuck_residue_bisim`), but occupies physical storage in RSpace if not elided. |
| **Token sweep**                      | A post-evaluation cleanup operation that removes internal fuel artifacts (authorization markers, non-billable routing markers, and unfired gates) from the deploy's signature channel(s) in the tuple space. Runs after successful evaluation and before the hard checkpoint. Deterministic across all validators. |
| **Per-deploy signature scoping**     | The practice of deriving a deploy's signature channel from a domain-separated hash of the deploy's cryptographic signature (`blake2b256("f1r3node:cost-accounted-rho:deploy-signature:v1" || deploy.sig)`), ensuring that each deploy's fuel tokens are isolated on a unique channel and cannot leak to other deploys. |
| **Soft checkpoint**                  | An in-memory snapshot of the HotStore state taken before deploy evaluation. Used to revert all tuple-space changes on deploy failure. See `SoftCheckpoint` in `rspace++/src/rspace/checkpoint.rs`.              |

---

## 3. The Baseline Externalized Architecture

### 3.1 The Externalized Cost Model

The F1R3Node interpreter (in this repository) uses an **externalized** cost model: phlogiston accounting is performed by wrapper layers that sit between the evaluator and the tuple space, rather than being embedded in the calculus itself. The architecture consists of three components.

**Component 1: `CostManager`** (`rholang/src/rust/interpreter/accounting/mod.rs:17`)

The `CostManager` is a shared mutable phlogiston counter protected by a standard mutex, with a bounded log of recent charges:

```rust
pub struct CostManager {
    state: Arc<Mutex<Cost>>,             // remaining phlogiston
    log: Arc<Mutex<VecDeque<Cost>>>,
    max_log_entries: usize,
}
```

Its sole mutation method is `charge(amount: Cost)` (`accounting/mod.rs:43`+):

1. Lock the mutex on `state` (one fallible lock — `Result<MutexGuard, …>`).
2. If `state.value < 0`, raise `OutOfPhlogistonsError` (pre-check).
3. Subtract `amount.value` from `state.value`.
4. Append `amount` to the log (truncating at `max_log_entries`).
5. If `state.value < 0` after subtraction, raise `OutOfPhlogistonsError` (post-check).

The two-check pattern (steps 2 and 5) mirrors the original Scala implementation: pre-check rejects further charges once the balance is exhausted, post-check catches the case where the current charge drives it below zero. Both checks happen inside a single critical section, so there is no race window between them.

**Component 2: `Cost` and the cost functions** (`rholang/src/rust/interpreter/accounting/costs.rs`)

The `Cost` struct pairs an `i64` value with an operation label:

```
struct Cost {
    value:     i64,
    operation: Cow<'static, str>,
}
```

The two storage cost functions are:

```
FUNCTION storage_cost_produce(channel: Par, data: ListParWithRandom) -> Cost:
    RETURN Cost(storage_cost([channel]).value + storage_cost(data.pars).value,
                "produces storage")

FUNCTION storage_cost_consume(channels: Vec<Par>, patterns: Vec<BindPattern>,
                              continuation: TaggedContinuation) -> Cost:
    LET body_cost = IF continuation has ParBody with body THEN
                        storage_cost([body])
                    ELSE
                        Cost(0)
    RETURN Cost(storage_cost(channels).value + storage_cost(patterns).value
                + body_cost.value,
                "consume storage")
```

where `storage_cost(items)` sums the protobuf-encoded lengths of the items.

The critical observation: `storage_cost_produce` sums `|channel| + |data|`, while `storage_cost_consume` sums `|channels| + |patterns| + |body|`. These are structurally different quantities measuring different things: the produce cost measures the data being sent, while the consume cost measures the patterns and continuation body being registered. For any non-trivial COMM event, these values differ.

**Component 3: `ChargingRSpace`** (retired pre-migration role)

`ChargingRSpace` wraps the raw `ISpace` trait with charging logic:

- On `produce(channel, data, persist)`:
  1. Charge `storage_cost_produce(channel, data)`.
  2. Call the underlying `space.produce(...)`.
  3. Call `handle_result(result, TriggeredBy::Produce{...}, cost)`.

- On `consume(channels, patterns, continuation, persist, peeks)`:
  1. Charge `storage_cost_consume(channels, patterns, continuation)`.
  2. Call the underlying `space.consume(...)`.
  3. Call `handle_result(result, TriggeredBy::Consume{...}, cost)`.

The retired `handle_result` function is the locus of the externalized model's per-COMM bookkeeping. When a COMM event fires (i.e., `result` is `Some`), it issues refunds for the storage that is no longer needed and charges for event storage:

```
FUNCTION handle_result(result, triggered_by, cost):
    MATCH result:
        Some((cont, data_list)) =>
            refund_for_consume = storage_cost_consume(cont.channels,
                                                      cont.patterns,
                                                      cont.continuation)
            refund_for_produces = SUM over data_list of
                storage_cost_produce(channel_i, data_i)

            cost.charge(-refund_for_consume)   // negative = refund
            cost.charge(-refund_for_produces)   // negative = refund
            IF last_iteration:
                cost.charge(event_storage_cost(channels_count))
            cost.charge(comm_event_storage_cost(cont.channels.len()))

        None =>
            cost.charge(event_storage_cost(channels_count))
```

The net cost of a COMM event is therefore:

```
net = (initial_charge) - (refund_for_consume) - (refund_for_produces)
      + (event_storage_cost) + (comm_event_storage_cost)
```

When a produce fires first (a matching consume is already waiting), the initial charge is `storage_cost_produce`. When a consume fires first (a matching produce is already waiting), the initial charge is `storage_cost_consume`. The pre-charge / refund / unified-COMM dance is precisely what makes the net cost order-independent: the unified-COMM charge replaces whichever pre-charge was applied first, refunds cancel the storage that is no longer in the tuple space, and the final accounting depends only on what the COMM produced — not on which side triggered it. This is the order-independent strategy described by the pre-migration evaluation-order notes and preserved here as historical context.

### 3.2 Why Externalize Is Suboptimal

The baseline externalized strategy is correct, but three structural weaknesses motivate the upgrade to an internalized model.

**(a) No consensus-critical formal proof of cost determinism.** The order-independence of `ChargingRSpace` is established empirically by the test suite: `cost_should_be_deterministic` (`rholang/tests/accounting/cost_accounting_spec.rs:323`) re-runs each contract 20 times and asserts identical cost; `cost_should_be_repeatable_when_generated` (line 344) runs 10,000 randomly-generated contracts; `peek_with_parallel_produce_should_have_deterministic_replay_cost` (line 474) directly guards the peek-with-parallel-produce class that is most sensitive to scheduling. This is strong sampling, but not a theorem. A bug in `handle_result` that only manifests under a previously-untested combination of persistent produces, multi-channel joins, and peek indices could escape the test suite and re-introduce cost divergence between validators. The internalized model upgrades this to a **machine-checked theorem** (`ca_cost_deterministic` in `formal/rocq/cost_accounted_rho/Confluence.v`) — proven via strong normalization plus local confluence, composed through Newman's lemma — that holds for every reachable state, not just the sampled ones.

**(b) `handle_result` is intricate per-COMM bookkeeping.** The retired pre-charge / refund / unified-charge dance required careful accounting that interacted non-trivially with other RSpace features. Concrete instances of this historical complexity include: (i) identity-based filtering to issue refunds only for the produce instance actually removed by the COMM, distinguishing it from persistent siblings; (ii) persist-vs-linear differential treatment so persistent produces/consumes were not refunded when they triggered a COMM but remained in the tuple space; (iii) peek-disposition logic for peeking consumes that remove the matched continuation but leave the matched produce in place. Each of these can be made correct, but each adds a maintenance surface that a future contributor must reason about when modifying the cost layer or the RSpace API. The internalized model collapses all of this to a single invariant: each source-token consumption event is billed exactly once. There is no per-application-COMM refund logic, no persist-vs-linear differentiation in the cost layer, no peek-disposition special case — none of the bookkeeping mechanisms exist because the cost is paid at billable source-token gates, not at every produce/consume.

**(c) No composable metering primitives.** The externalized model meters total deploy phlogiston monolithically: every deploy carries a single `CostManager` instance with a single `i64` balance, and the only way to spend phlogiston is to call `cost.charge(amount)` from inside the interpreter or RSpace wrapper. This is sufficient for deploys with a single principal, but it cannot natively express paper §6.2's composable metering patterns: (i) **rate-limited markets** where a contract publishes a budget on a public channel and consumers can spend against it without compromising the principal's overall limit; (ii) **joint-account spending** where two principals' fuel pools are pooled into a single compound signature for the duration of a multi-step protocol; (iii) **prepayment / receiver-pays** where a sender includes fuel with a message so the receiver doesn't pay to handle it; (iv) **delegation / forwarding** where principal A authorizes principal B to spend up to N tokens on A's behalf. Implementing any of these under the externalized model requires a bespoke runtime layer that mediates between the user's intent and `CostManager`. Under the internalized model, signatures and tokens are first-class language constructs — each pattern is just a particular composition of signed processes, Split mediators, and Join mediators, with no runtime support beyond the translation pass itself.

### 3.3 UML Sequence Diagram: F1R3Node's Current Deploy Execution Flow

The following diagram shows F1R3Node's baseline evaluation of a deploy containing two parallel COMM interactions, with `ChargingRSpace` wrapping each `produce`/`consume` to apply the pre-charge / refund / unified-COMM strategy. The cost is order-independent under this strategy; the diagram shows the operational flow.

![Deploy execution sequence — F1R3Node's baseline evaluation of a deploy with two parallel COMM interactions. End-to-end: Deployer submits the deploy, Evaluator dispatches through ChargingRSpace → RSpace, CostManager accumulates charges and refunds, and the final cost is queried and returned to the Deployer. Each spawned per-term future routes through ChargingRSpace, which pre-charges storage cost on produce/consume, refunds on COMM match, and applies a unified COMM cost. The final cost is deterministic by the ChargingRSpace strategy.](diagrams/deploy-execution-sequence.svg)

(*Source: [`diagrams/deploy-execution-sequence.puml`](diagrams/deploy-execution-sequence.puml) — render with `plantuml -tsvg …`.*)

Key observations from the diagram:
- The sequence is **end-to-end**: it begins with `Deployer → Evaluator: deploy(P ∣ Q, initial_cost)` and ends with `Evaluator → Deployer: deploy result, cost = c` after `CostManager.total_cost()` is queried.
- `storage_cost_produce`, `storage_cost_consume`, and the unified COMM charge are the three cost categories. On a COMM match, `ChargingRSpace::handle_result` refunds the storage that is no longer in the tuple space and applies the unified COMM cost; the order in which produce or consume triggers the match does not affect the net.
- The Evaluator previously spawned each per-term future via `tokio::spawn` and awaited the corresponding `JoinHandle`s sequentially. The migration drains branch tasks through a single `FuturesUnordered` driver (Section 5.4), preserving parallel branch execution while removing the sequential join barrier. Per-channel synchronization lives inside the `ISpace` implementation (HotStore / RSpace), not in the eval loop — the eval loop holds `RhoISpace = Arc<Box<dyn ISpace<…> + Send + Sync>>`, not a global mutex.
- **RSpace semantics**: each `produce`/`consume` either matches immediately or is stored pending a future counterpart. A parked continuation is a *registration* (waiting-pattern stored in the tuple space), not a match event. Persistence to LMDB is an implementation detail of `RSpace` and is abstracted behind the `ISpace` trait.
- The `no match → continuation parked` outcome on f₁'s consume makes it explicit that the continuation survives in the tuple space awaiting a future produce — it is not discarded.
- Cost is determined by the deploy, not by scheduling. This proposal further upgrades the guarantee from empirical (the existing deterministic-cost tests in `rholang/tests/accounting/cost_accounting_spec.rs`) to mechanized (`ca_cost_deterministic` in `Confluence.v`); see §4.

---

## 4. The Cost-Accounted Rho Calculus Solution

### 4.1 The Translation

The cost-accounted rho calculus extends the pure rho calculus with three new syntactic categories — signatures, tokens, and systems — and re-expresses computation cost as token consumption [4]. A compositional translation defined by four mutually recursive functions maps cost-accounted terms back into the pure rho calculus, where cost accounting happens naturally via COMM events on dedicated channels.

**Signature Translation** `N⟦·⟧ : sig → name`

Signatures become channels. The three constructors map as follows:

| Signature      | Translation                            | Intuition                                                                                                                                                                         |
|----------------|----------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `SUnit`        | `N⟦SUnit⟧ = @0`                        | The unit signature maps to the channel whose underlying process is nil.                                                                                                           |
| `SHash(bs)`    | `N⟦SHash(bs)⟧ = @H_bs`                 | An atomic signature maps to the channel naming the canonical process `H_bs` determined by the digital signature's byte string `bs`. The function `H` is required to be injective. |
| `SAnd(s₁, s₂)` | `N⟦SAnd(s₁, s₂)⟧ = @(*N⟦s₁⟧ ∣ *N⟦s₂⟧)` | A compound signature dereferences both component channels in parallel and re-quotes the result.                                                                                   |

The key property of `N⟦·⟧` is that it produces **closed** names: no de Bruijn variables appear in the output. This is proven as `N_tr_closed` in the Rocq formalization, and it is load-bearing because it ensures that substitution during COMM events does not interfere with signature channels.

**Token Translation** `T⟦·⟧ : token → proc`

Tokens become parallel outputs on signature channels:

| Token         | Translation                    | Intuition                                                                                                                        |
|---------------|--------------------------------|----------------------------------------------------------------------------------------------------------------------------------|
| `TUnit`       | `T⟦TUnit⟧ = 0`                 | The empty token translates to the stopped process. No fuel remains.                                                              |
| `TGate(s, t)` | `T⟦TGate(s, t)⟧ = N⟦s⟧!(T⟦t⟧)` | A gated token becomes an output on the signature channel `N⟦s⟧`, carrying the translation of the remaining token `t` as payload. |

A token `TGate(s₁, TGate(s₂, TUnit))` with two fuel units translates to:

```
N⟦s₁⟧!(N⟦s₂⟧!(0))
```

This is a single output on `N⟦s₁⟧` whose payload is itself an output on `N⟦s₂⟧` whose payload is nil. The nesting encodes the fuel stack.

**Runtime representation.** The nested-stack representation is the mathematical model used in the Rocq formalization. At production phlogiston limits (90K-1M units), creating a deeply nested `POutput` term would be impractical and would also create a false sequential bottleneck. The runtime therefore uses a bounded-memory **token budget** (Section 5.3): it stores the deploy's remaining fuel as an integer, records deterministic source-event descriptors for fired recursive gates, and commits billable events in canonical descriptor order. For atomic chains this is a coalesced implementation of the same token consumption relation; for compound signatures, derived Split/Join routing markers are internal and non-billable so the source-token count remains the authoritative cost.

Like `N⟦·⟧`, the token translation produces **closed** terms (`T_tr_closed` in Rocq). This means token translations are invariant under substitution and lifting, which is essential for the simulation proofs.

**Signed Process Translation** `P⟦P, s⟧ : (proc, sig) → proc`

A signed process `P^s` is translated to a fuel-gated input:

For atomic signatures (`SUnit`, `SHash(bs)`):
```
P⟦P, s⟧ = for(t ← N⟦s⟧){ lift(P, 1, 0) | *t }
```

For compound signatures (`SAnd(s₁, s₂)`):
```
P⟦P, SAnd(s₁, s₂)⟧ = for(t₁ ← N⟦s₁⟧) for(t₂ ← N⟦s₂⟧)
                         ( lift(P, 2, 0) ∣ *t₁ ∣ *t₂ )
```

The `lift(P, d, 0)` operation shifts de Bruijn indices in `P` by `d` to account for the `d` binders introduced by the fuel gate's input prefix(es). Without this lift, substitution for the fuel token variable could accidentally capture variables in `P` that have nothing to do with the fuel gate.

Operationally, the fuel gate works as follows:
1. The input prefix `for(t ← N⟦s⟧)(…)` blocks until a matching output arrives on channel `N⟦s⟧`.
2. When a token `N⟦s⟧!(payload)` is present, the COMM rule fires: the bound variable `t` (de Bruijn index 0) is replaced with `@payload`.
3. The body `lift(P, 1, 0) ∣ *(@payload)` reduces: `lift(P, 1, 0)` evaluates to `P` (by the cancellation lemma `subst_lift_zero`), and `*(@payload)` evaluates to `payload`.
4. The result is `P ∣ payload`, where `payload = T⟦t'⟧` is the translated remainder of the token stack.

The crucial safety property (`fuel_gate_stuck_isolated` in Rocq): the fuel gate is an input prefix in isolation. By the definition of the rho calculus operational semantics, an input prefix cannot reduce without a matching output. Therefore, **the body `P` cannot execute without a matching token.** This is the formal proof of fuel-gate safety.

**System Translation** `S⟦·⟧ : system → proc`

Systems are translated component-wise:

| System          | Translation                       |
|-----------------|-----------------------------------|
| `SSigned(P, s)` | `S⟦SSigned(P, s)⟧ = P⟦P, s⟧`      |
| `SToken(t)`     | `S⟦SToken(t)⟧ = T⟦t⟧`             |
| `SPar(S₁, S₂)`  | `S⟦SPar(S₁, S₂)⟧ = S⟦S₁⟧ ∣ S⟦S₂⟧` |

The third clause is what makes the translation **compositional** (`S_tr_compositional` in Rocq): the translation of a parallel composition is the parallel composition of the translations. This means cost accounting distributes naturally over parallel decomposition.

**Mediator Processes: Split and Join**

Compound signatures require mediation between the compound channel `N⟦s₁ & s₂⟧` and the pair of atomic channels `(N⟦s₁⟧, N⟦s₂⟧)`. The paper defines two mediator processes for this purpose (Definition 4.1 and Definition 4.2 in [4]):

```
Split(s₁, s₂) = for(t ← N⟦s₁ & s₂⟧)
                    ( N⟦s₁⟧!(0) ∣ N⟦s₂⟧!(*t) )

Join(s₁, s₂)  = for(t₁ ← N⟦s₁⟧) for(t₂ ← N⟦s₂⟧)
                    ( N⟦s₁ & s₂⟧!(*t₁ ∣ *t₂) )
```

`Split` (Definition 4.1 in [4]) takes a compound token and produces two atomic tokens. `Join` (Definition 4.2 in [4]) takes two atomic tokens and produces a compound token. These are **persistent infrastructure processes** deployed once and reused across all deploys.

### 4.2 Why Cost Is Deterministic (Intuition)

Cost, in the internalized model, is **source-token consumption**. The
source calculus assigns a fixed token decrement to each of Rules 1-5:
Rules 1, 3, and 4 consume one source token; Rules 2 and 5 consume two.
The total cost of a deploy is the number of billable source-token
consumption events across the deploy's lifetime, not the number of
pure-rho COMM steps introduced by Split/Join routing. Three observations
make this number deterministic:

1. **Structural ordering of a deploy's gates.** The token-chain
   encoding `T⟦σ:T'⟧ = N⟦σ⟧!(T⟦T'⟧)` places at most one token message
   on any signature channel at a time. Each gate firing dequotes the
   next token via `*t` and only *then* is the next gate reachable.
   Within one deploy, validators therefore see the same sequence of
   billable source-token events — not by agreement, but because the process
   structure admits exactly one reduction path for the fuel-gate
   layer.

2. **Additivity of token count over parallel composition.**
   `‖SPar(S₁, S₂)‖ = ‖S₁‖ + ‖S₂‖`. When two independent fuel gates
   fire in different orders, the total tokens consumed is the sum
   either way. Unlike the externalized model, where
   `storage_cost_produce ≠ storage_cost_consume` makes the
   `E₁`-first and `E₂`-first orderings diverge, here every ordering
   yields the same total because each event's contribution is a
   fixed positive integer.

3. **Confluence.** Multi-deploy blocks reduce via the cost-accounted
   calculus, which is strongly normalizing and confluent. Any two
   reduction sequences from the same start state reach terminal
   states with identical `‖·‖`, so
   `total_consumed = ‖S_initial‖ − ‖S_terminal‖` is a function of
   endpoints alone.

**Channel separation.** User code (possibly containing multi-channel
joins such as `for(x <- ch1; y <- ch2){P}`) runs strictly inside a
fuel-gate body and communicates on application channels; fuel gates
themselves live on dedicated signature channels `N⟦s⟧` derived via
injective `hash_process`. The Rocq development proves the syntactic
separation facts it models; a Rust implementation must additionally
construct these channels as unforgeable runtime names so user code cannot
alias or synthesize them. Application-level reductions — including joins
— must not be able to interfere with billable source-token matches.

**Formal statements.** Each of the three observations is stated as
a theorem in the verification doc:

- Token conservation & strict decrease — verification doc §9.1
  (`token_monotone_step`, `token_monotone_reachable`,
  `token_strictly_decreases`).
- Step determinism for a single deploy — verification doc §7.1,
  §10.3 (`ca_step_deterministic`, `single_token_path_unique`).
- Confluence and cost determinism — verification doc §7.1, §11.2
  (`ca_strongly_normalizing`, `ca_local_confluence`, `newman`,
  `ca_confluent`, `ca_cost_deterministic`).
- Fuel-event multiset determinism — verification doc §9.7
  (`fuel_events_consumed_perm`).
- Channel separation — verification doc §10.5, §11.1
  (`N_tr_is_Quote`, `fuel_gate_no_app_channel_overlap`).
- TLA+ finite-state validation of all the above — verification doc
  §10.3 `CostDeterminism` invariant, checked exhaustively on the
  most comprehensive model (12,960 distinct states across 7
  processes and Join mediators).

### 4.3 Why FuturesUnordered Becomes Safe

With internalized cost accounting, the eval loop no longer needs `ChargingRSpace`'s pre-charge / refund / unified-COMM bookkeeping. The baseline externalized model in f1r3node-rust achieved order-independence via that bookkeeping at the cost of per-task overhead; the internalized model achieves it via the mathematics of token conservation, with no broad charging wrapper. Every billable source-token event consumes exactly the amount prescribed by Rules 1-5, regardless of which other gates or routing mediators have already fired.

The simplified eval loop becomes:

```rust
async fn eval(par: Par, env: &Env, rand: Blake2b512Random) -> Result<()> {
    let terms = flatten_par(&par);
    let mut futures = FuturesUnordered::new();
    for (i, term) in terms.iter().enumerate() {
        futures.push(eval_term(term, env, rand.split(i)));
    }
    while let Some(result) = futures.next().await {
        result?;
    }
    Ok(())
}
```

This is the simplified version referenced in the executive summary. There is no Phase 1 / Phase 2 distinction. There is no dispatch queue. There is no serialization barrier. Every sub-term is evaluated concurrently, and COMM bodies are dispatched as soon as their fuel gates fire, in whatever order the runtime selects. The cost is deterministic because it depends only on how many tokens are consumed, not on the order of consumption.

The eval-loop scheduling property is verified directly by the
`EvalScheduling.tla` finite-state model (see verification doc §10.1,
§10.3, §10.4): for `N` bodies each requiring one fuel token, every
permutation of body execution yields the same total cost, and TLC
confirms this across all 16 reachable states of a 3-body system. The
same model tracks `extCost` (the externalized model's cost) and
demonstrates that an idealized externalized model with `StorageCostA ≠ StorageCostB` and no compensating refund logic diverges across permutations — illustrating why an order-independent strategy at the externalized layer requires careful refund bookkeeping (which `ChargingRSpace` provides today via pre-charge / refund / unified-COMM, and which the internalized model makes unnecessary by metering source-token consumption instead).

### 4.4 Diagram: The Recursive Metered Gate Protocol

The following diagram shows the lifecycle of one selected source step
under the recursive metering kernel:

![Recursive metered gate protocol — end-to-end lifecycle of one selected source step. The Deployer submits the Rholang source with its signature; the metering pipeline parses, normalizes, annotates, and builds a well_reflected metered state. The Evaluator executes continuation-keyed gates through TokenBudget and RSpace. The five stages are explicit: source-event discovery, metered gate registration, COMM firing with source-token accounting, continuation resume, and registration of any next ready source step. The final cost is queried from TokenBudget and returned to the Deployer.](diagrams/fuel-gate-protocol.svg)

(*Source: [`diagrams/fuel-gate-protocol.puml`](diagrams/fuel-gate-protocol.puml) — render with `plantuml -tsvg …`.*)

The protocol has five stages:

1. **Authorization deposit** *(cost: 0)*. The evaluator deposits one authorization marker for the selected source step on the continuation channel `Quote(K)`. `RSpace` stores the output record and returns `stored, pending match`. No fuel is charged — the marker is data in transit.

2. **Metered gate registration** *(cost: 0)*. The recursive gate `recursive_metered_gate(K)` is registered as a `consume` event on the same continuation channel. `RSpace` stores the continuation registration and returns `stored, waiting`. No fuel is charged — the gate is just a waiting pattern.

3. **Metered gate fires** *(cost: one billable source-token event with deterministic weight)*. `RSpace` matches the authorization and gate on `Quote(K)` and signals `TokenBudget.reserve_canonical(...)` after the metering kernel records the event identity from the deploy id, source path, redex id, local index, billable kind, optional primitive descriptor, and weight. The retired `produce` and `consume` are removed from the tuple space as a COMM event. `RSpace` returns the continuation `K ∣ PNil` to the Evaluator.

4. **Continuation resumes** *(no additional fuel charge)*. The Evaluator resumes with `K`, which is itself a recursively metered image of the source successor. The Rocq theorem `recursive_metered_gate_per_step_reverse` proves that every rho step out of the gate lands in a state structurally equivalent to `K`.

5. **Next ready source step** *(charged only if its gate fires)*. If the successor contains another enabled source COMM interaction, the evaluator installs a fresh `recursive_metered_gate(K')` for that source step. Independent branches can register their gates concurrently; whichever branch becomes ready first can fire without imposing a global serialization barrier. Each fired recursive gate corresponds to exactly one `ca_step` by `well_reflected_backward_reflection`.

After all evaluation completes, the Evaluator queries `TokenBudget.total_cost()` and returns `deploy result, cost = consumed source-token units` to the Deployer. Cost accounting in the implementation target is therefore localized to fired recursive metered gates; pending registrations, continuation scheduling, and Split/Join routing markers are not billable by themselves.

---

## 5. Architectural Changes

**Important: no Rholang syntax changes.** All changes described in this
section are internal to the compiler and runtime. The Rholang surface
language is completely unchanged — existing Rholang source code requires
zero modifications. The types, translation pass, and cost model described
below operate inside the compiler pipeline, between the normalizer and the
reducer, and are invisible to the Rholang programmer.

### 5.1 New Internal Types

The cost-accounted rho calculus uses three algebraic types that mirror the
Rocq formalization. These are **internal compiler intermediate
representations** (IR), not Rholang surface syntax. They exist only inside
the translation pipeline and are never exposed to the Rholang programmer:

```rust
/// A digital signature, used to authorize fuel consumption.
/// Corresponds to Rocq type `sig` in CostAccountedSyntax.v.
enum Sig {
    /// The trivial unit signature. Authorizes any fuel gate.
    Unit,
    /// An atomic signature derived from a byte string (hash of a key).
    Hash(Vec<u8>),
    /// A compound signature requiring both s1 AND s2.
    And(Box<Sig>, Box<Sig>),
}

/// A fuel token.
///
/// The Rocq formalization uses a right-nested stack (`TGate(s, TGate(s, ... TUnit))`)
/// for proof simplicity. The runtime uses a flat counter for practical efficiency —
/// at production phlogiston limits (90K–1M), a nested stack would create an
/// impractically deep AST. The `TokenBudget` (Section 5.3) implements the
/// bounded-memory accounting strategy that coalesces the nested-stack model.
///
/// Corresponds to Rocq type `token` in CostAccountedSyntax.v (mathematical model).
enum Token {
    /// The empty token. No fuel remains.
    Unit,
    /// Runtime representation: a flat counter of remaining fuel units,
    /// all guarded by the same signature. Used by `TokenBudget`.
    Count { sig: Sig, remaining: u64 },
    /// Mathematical representation: one unit of fuel guarded by signature `sig`,
    /// with remaining balance `rest`. Used in the Rocq formalization only.
    Gate { sig: Sig, rest: Box<Token> },
}

/// A signed process: a Rholang process paired with its authorizing signature.
/// Corresponds to Rocq type `system` in CostAccountedSyntax.v.
enum SignedProcess {
    /// A process sealed under a signature.
    Signed { process: Par, sig: Sig },
    /// A free-standing fuel token.
    Token(Token),
    /// Parallel composition of two signed processes.
    Par(Box<SignedProcess>, Box<SignedProcess>),
}
```

Additionally, a `SignatureChannel` type wraps the translation `N⟦s⟧`:

```rust
/// A channel derived from a signature. The name used for fuel-gate COMM events.
struct SignatureChannel {
    /// The Par representation of the channel @H_s.
    par: Par,
}

impl SignatureChannel {
    /// N⟦s⟧: translate a signature to a channel name.
    fn from_sig(sig: &Sig) -> Self { /* ... */ }
}
```

### 5.1.1 Discharging the Hash Process Hypotheses

The entire Rocq formalization is parameterized over a single function `hash_process : list bool → proc` with three structural/cryptographic hypotheses on it. Every translation-side theorem that reasons about hash-derived signature channels (contextual forward reachability, per-step reverse, atomic and compound bisimulation, fuel-gate safety for hashed signatures) depends on these properties being satisfied by the concrete implementation. Consensus-critical results — `ca_cost_deterministic`, `ca_step_deterministic`, `token_monotone_*`, `fuel_events_consumed_perm` — are unconditional and do not reference `hash_process` at all (see verification doc §12.1 per-theorem dependency table). This is the ONLY undischarged parameter in the formalization; there are no axioms, no active admissions, and no other assumptions.

The proposed implementation uses the existing Rholang `GPrivate` unforgeable name mechanism:

```
HASH_TO_PROCESS(bs):
    RETURN PDeref(Quote(GUnforgeable(GPrivate(blake2b256(bs)))))
```

The encoding parameter and three hypotheses are discharged as follows:

**Injective** (`hash_process_injective`): The function must map distinct byte strings to distinct processes. `blake2b256` is a cryptographic hash function with 256-bit output and collision resistance as a design property. Distinct byte strings `bs1 ≠ bs2` produce distinct hashes `blake2b256(bs1) ≠ blake2b256(bs2)` (with negligible collision probability bounded by the birthday paradox at `2^{-128}`). Distinct hashes produce distinct `GPrivate` values, hence distinct `GUnforgeable` names, hence distinct `PDeref(Quote(...))` processes. Injectivity holds under the standard cryptographic assumption that `blake2b256` is collision-resistant.

**Closed** (`hash_process_closed`): The output must have no free de Bruijn indices — that is, it must be a closed term. `GUnforgeable(GPrivate(...))` is an atom in the Rholang name grammar: unforgeable names carry no variable references and no sub-terms with binders. Wrapping in `Quote(...)` produces a closed name, and `PDeref(...)` of a closed name is a closed process. Formally, `free_vars(PDeref(Quote(GUnforgeable(GPrivate(h))))) = {}` for any hash `h`.

**Head count = 1** (`hash_process_head_count_one`): The output must have `head_count = 1` — it must not be a parallel composition. `PDeref(Quote(...))` is a single `PDeref` constructor applied to a single `Quote` name. It is not a `PPar`, so its head count is exactly 1. This ensures that signature channels derived from hashed processes cannot be confused with channels derived from compound signatures (which use parallel composition `*N⟦s₁⟧ ∣ *N⟦s₂⟧`).

**Well-typed**: The output must be a valid Rholang `Par` term suitable for quotation into a channel name via `N⟦hash(σ)⟧ = @(hash_to_process(σ))`. The construction `PDeref(Quote(GUnforgeable(GPrivate(blake2b256(bs)))))` is well-formed: `blake2b256(bs)` produces a 32-byte array, `GPrivate` wraps it as an unforgeable private name, `GUnforgeable` lifts it into the name grammar, `Quote` produces a name, and `PDeref` produces a process. Quoting the result with `@(...)` yields a valid channel name that can serve as the target of fuel-gate inputs and token outputs.

### 5.2 The Recursive Metering Pass

The implementation pass runs after Rholang parsing and normalization but
before evaluation. It does not pre-expand the whole deploy into the raw
legacy `S_tr` image. Instead, it keeps a cost-accounted `SignedProcess`
IR and drives evaluation through the recursively metered invariant proved
as `well_reflected_backward_reflection`.

```
FUNCTION meter_next(sys: SignedProcess) -> Option<MeteredGate>:
    LET step = choose_enabled_ca_step(sys)  // may select either side of SPar
    MATCH step:
        None =>
            RETURN None
        Some { successor, continuation_key } =>
            RETURN recursive_metered_gate(continuation_key, successor)

FUNCTION recursive_metered_gate(key: ContinuationKey,
                                successor: SignedProcess) -> MeteredGate:
    RETURN Gate {
        channel: Quote(key),
        on_match: resume_with(successor)
    }

FUNCTION translate_token(t: Token) -> Par:
    MATCH t:
        Unit         => PNil
        Gate(s, t')  => POutput(sig_channel(s), translate_token(t'))

FUNCTION sig_channel(s: Sig) -> Name:
    MATCH s:
        Unit         => Quote(PNil)
        Hash(bs)     => Quote(hash_to_process(bs))
        And(s1, s2)  => Quote(PPar(PDeref(sig_channel(s1)),
                                    PDeref(sig_channel(s2))))
```

`translate_token` and `sig_channel` are still the concrete counterparts
of the paper's `T⟦·⟧` and `N⟦·⟧` functions; they are used to derive
signature-channel names, token markers, and Split/Join routing markers.
`P_tr` and `S_tr` remain proof and traceability artifacts, but the
runtime object that must satisfy whole-system reflection is the recursive
metered gate relation.

The pass sits in the deploy pipeline as follows. Note that the Rholang
source code is **unchanged** — the new stages operate on the compiler's
internal `Par` representation, not on the source text:

```
  User Rholang Source              ◄── unchanged; no new syntax
          │
          v
  ┌───────────────┐
  │ Parser +      │  (existing, unchanged)
  │ Normalizer    │
  └───────────────┘
          │
          v
     Plain Par                     ◄── standard Rholang AST
          │
          v
  ┌───────────────┐
  │ Signature     │  (NEW: internal compiler pass)
  │ Annotator     │  Wraps Par in SignedProcess using deployer's key
  └───────────────┘  from deploy metadata — not from Rholang source
          │
          v
   SignedProcess                   ◄── internal IR, never seen by programmer
          │
          v
  ┌───────────────┐
  │ Cost-Accounted│  (NEW: internal compiler pass)
  │ Metering Pass │  Builds recursive metered continuations
  └───────────────┘
          │
          v
   Metered Work Queue              ◄── standard Par gates plus continuations
          │
          v
  ┌───────────────┐
  │ Evaluator     │  (existing, but with FuturesUnordered)
  │ (reduce.rs)   │
  └───────────────┘
          │
          v
     Result + Cost
```

The Signature Annotator determines the appropriate signature for each
deploy based on the deploy's cryptographic signature, which is part of the deploy's
metadata (deployer public key, timestamp, term, signature) — **not** from the Rholang source
code. For most deploys, this will be `Sig::Hash(blake2b256("f1r3node:cost-accounted-rho:deploy-signature:v1" || deploy.sig))`, where `deploy.sig` is the deploy's Ed25519/Secp256k1 signature (unique per deploy by construction — it signs `hash(term) + timestamp` with the deployer's private key). Domain separation prevents raw signature bytes from being reused accidentally as another protocol hash. This ensures per-deploy signature channel isolation (see Section 5.8.1 for the rationale and security analysis). The deploy's phlogiston limit initializes the bounded token budget (Section 5.3); the implementation must not materialize one runtime object per phlo.

**System deploy exemption.** System deploys (genesis, slash, close-block, heartbeat), identified by `is_system_deploy_id(deploy_id)`, are exempt from the user-deploy cost-accounting translation. They run directly as plain `Par` terms under an explicit unmetered/no-op budget, not under the legacy `CostManager` framework. The Signature Annotator skips the translation pipeline for system deploy execution, while user-deploy fee settlement still uses system deploys as described in Section 5.9.2. Slash system deploys remain outside user metering, but their cost-invalid evidence must be current at the slashing boundary and authorized by the parent pre-state bond view.

### 5.3 New Cost Model: TokenBudget

The `TokenBudget` replaces the consensus-charging role of both `CostManager` and `ChargingRSpace` for user deploys. Instead of tracking mutable remaining phlogiston with arbitrary `charge(amount)` calls and refunds, it records **billable source-token events** emitted by the recursive metering kernel. Raw pure-rho COMM count is not the cost metric: Split/Join and other routing COMMs can occur while consuming zero additional source tokens.

```
struct TokenBudget {
    initial_tokens: u64,
    consumed:       AtomicU64,
}

struct BillableTokenEvent {
    deploy_id:   [u8; 32],
    source_path: SourcePath,
    redex_id:    RedexId,
    weight:      u64,
    kind:        BillableKind,
}

impl TokenBudget {
    FUNCTION new(phlogiston_limit: u64) -> Self:
        Self {
            initial_tokens: phlogiston_limit,
            consumed: AtomicU64::new(0),
        }

    FUNCTION reserve_canonical(&self, event: BillableTokenEvent) -> Result<()>:
        // Called only after source events have been ordered by canonical descriptor.
        LET prev = self.consumed.fetch_add(event.weight, Ordering::AcqRel)
        IF prev + event.weight > self.initial_tokens:
            RETURN Err(OutOfPhlogistonsError)
        RETURN Ok(())

    FUNCTION total_cost(&self) -> u64:
        RETURN self.consumed.load(Ordering::Acquire)

    FUNCTION remaining(&self) -> u64:
        RETURN self.initial_tokens - self.total_cost()
}
```

Key differences from `CostManager`:

1. **No mutex on the hot path.** Token consumption is an atomic weighted reservation after canonical source-event ordering. No global cost mutex is shared by all reducer tasks.

2. **No charges or refunds.** There is no `charge(amount)` / `charge(-refund)` dance. Cost goes in one direction: up by the deterministic weight of each billable source event.

3. **No broad `ChargingRSpace`.** The tuple space no longer pre-charges and refunds every `produce`/`consume`. A narrow metered-match observer remains required so accounting is driven by fired recursive gates, not by attempted deposits or ordinary application COMMs.

4. **Deterministic by construction.** Since cost is the sum of source-token event weights, and the source calculus proves confluence and cost determinism, the total is independent of scheduling order. The canonical event order is needed only to make out-of-phlogiston boundaries reproducible when the budget is insufficient for all enabled work.

**Bounded-memory token budget.** The mathematical model (Section 4.1) represents a deploy's fuel as a right-nested token stack `TGate(s, TGate(s, ... TUnit))` of depth `n`. Translating this directly via `T⟦·⟧` would produce a deeply nested `POutput` term with `n` levels — impractical at production phlogiston limits (90K-1M) and hostile to parallelism. The Rust implementation must instead coalesce that stack into `TokenBudget { initial_tokens, consumed }` and treat each fired recursive gate as a finite macro of source-token consumption events:

1. Reducer workers may discover enabled source redexes concurrently.
2. Each candidate produces a deterministic `BillableTokenEvent` descriptor and weight.
3. The metering kernel commits ready events in canonical descriptor order for that deploy.
4. `TokenBudget::reserve_canonical` atomically reserves the event's weight.
5. If the reservation would exceed `initial_tokens`, evaluation raises `OutOfPhlogistonsError` at that canonical event and rejects later events.

The body after an atomic fuel gate fires becomes `P ∣ *(@PNil) ≡ P ∣ PNil ≡ P` (the stuck residue is elided per Section 5.8.3). For atomic deploy-signature chains, the budgeted implementation is bisimilar to the nested-stack model at the cost boundary: both accept exactly the same prefix of canonical source-token events and report the same consumed total. Compound signatures add non-billable routing markers for Split/Join, but the billable count remains the source-token count proven by `TokenConservation.v`. The formal calculus uses nested stacks for proof simplicity; `RuntimeBudgetRefinement.v` proves the coalesced counter obligations used by the implementation: consumed/remaining conservation, successful weighted reservation, out-of-phlo boundary commitment, reset-from-token trace clearing, finalization-read cost trace windows, and replay-payload trace sensitivity.

**Rust implementation names.** In `f1r3node-rust`, the bounded-memory
`TokenBudget` is implemented as `RuntimeBudget`; the reducer-facing
recursive metering facade is `MeteredMachine`. `RuntimeBudget` owns the
atomic token counter, canonical event log, and out-of-phlogistons event
descriptor. `MeteredMachine` owns source-path context for reducer
branches and is the only user-deploy path for source-step, substitution,
and primitive-operation reservations. `DebruijnInterpreter` deliberately
does not carry a legacy `cost` field, so accidental old-style
`self.cost.charge(...)` calls fail at compile time.

The runtime must not bill an attempted gate registration. In asynchronous RSpace, the gate may not be registered yet, multiple continuations may contend, and persistent mediators may interleave. Therefore `reserve_canonical()` is called only after the recursive metering kernel has identified a fired billable source gate. Application-level COMM events remain uncharged by the old storage wrapper; their cost is represented by source-token events selected by the recursive metering kernel.

**Recursive metering kernel.** The business-critical implementation
target is not the raw legacy `S_tr` image by itself. A raw `P_tr` gate
can spend an outer token for a body that is source-terminal, so arbitrary
rho steps from that image do not always reflect to `ca_step`. The
verified target is `well_reflected` (`TranslationFaithfulness.v:4147`):
the evaluator chooses an enabled source step, installs a
continuation-keyed `recursive_metered_gate`, and after the gate fires it
re-enters the evaluator with the recursively metered continuation for
the source successor. This is an interpreter-style translation rather
than a monolithic pre-expansion. It maximizes parallelism because any
left or right branch step admitted by `ca_par_l` / `ca_par_r` can be
selected independently (`recursively_metered_parallel_left_enabled` and
`recursively_metered_parallel_right_enabled`), while each selected step
still carries an exact one-step reflection proof.

**Mapping `ca_step` rules to `TokenBudget` reservations.** Each of the five cost-accounted reduction rules (Appendix B) consumes a source-token count proven in `TokenConservation.v`. The implementation must bill that source count, not the number of pure-rho COMM steps in the translation:

| Rule | Signature shape                    | Token form      | Source tokens consumed | Billing rule |
|------|------------------------------------|-----------------|------------------------|--------------|
| 1    | Single `s`                         | `s:T`           | 1                      | Bill the direct source-token match. |
| 2    | Compound `s₁ & s₂`, split tokens   | `s₁:T₁ ∥ s₂:T₂` | 2                      | Bill one source-token event for each split token consumed. |
| 3    | Compound `s₁ & s₂`, combined token | `(s₁ & s₂):T`   | 1                      | Bill once when the combined source token is consumed; derived atomic routing markers are non-billable. |
| 4    | Split processes, combined token    | `(s₁ & s₂):T`   | 1                      | Bill once for the combined source token; subsequent translated gate firings carry non-billable metadata. |
| 5    | Split processes, split tokens      | `s₁:T₁ ∥ s₂:T₂` | 2                      | Bill each independent source-token match. |

Split and Join mediator firings are infrastructure — they redistribute tokens between compound and atomic channels but do not increment the token counter. Derived routing markers must carry internal non-billable metadata so Rules 3 and 4 cannot be overcharged by counting every translated COMM.

### 5.4 Simplified Eval Loop: Before and After

**Before — the pre-migration `eval_inner` shape** (with the retired `ChargingRSpace` role wrapping every `produce`/`consume` via `RhoISpace`):

```rust
// Each spawned future calls into ChargingRSpace internally, which
// pre-charges storage cost, calls the underlying ISpace produce/consume,
// then on COMM match refunds the now-removed storage and applies the
// unified COMM cost. The cost layer is correct but every spawned task
// pays the per-COMM bookkeeping overhead.
let futures: Vec<Pin<Box<dyn Future<Output = Result<(), InterpreterError>> + Send>>> =
    terms.iter().enumerate().map(|(index, term)| {
        let self_clone   = self.clone();
        let term_clone   = term.clone();
        let env_clone    = env.clone();
        let rand_split   = split(index.try_into().unwrap(), &terms, rand.clone());
        Box::pin(async move {
            self_clone.generated_message_eval(&term_clone, &env_clone, rand_split).await
        }) as _
    }).collect();

let handles: Vec<JoinHandle<Result<(), InterpreterError>>> =
    futures.into_iter().map(|fut| tokio::spawn(fut)).collect();

let mut flattened_results: Vec<InterpreterError> = Vec::new();
for handle in handles {
    match handle.await {
        Ok(Err(err))     => flattened_results.push(err),
        Err(join_err)    => flattened_results.push(InterpreterError::ReduceError(
                                format!("task panicked: {}", join_err))),
        Ok(Ok(()))       => {}
    }
}
```

**After (with internalized cost accounting):**

```rust
async fn eval_inner(par: Par, env: &Env, rand: Blake2b512Random) -> Result<()> {
    let terms = flatten_and_classify(&par);
    let mut futures = FuturesUnordered::new();
    for (i, term) in terms.iter().enumerate() {
        futures.push(async move { (i, eval_term(term, env, rand.split(i)).await) });
    }
    let mut errors = Vec::new();
    while let Some(result) = futures.next().await {
        match result {
            (_, Ok(())) => {}
            (i, Err(err)) => errors.push((i, err)),
        }
    }
    aggregate_in_stable_order(errors)
}
```

The differences from the pre-migration baseline:

1. **`FuturesUnordered` instead of sequential `JoinHandle` awaiting.** Sub-term branch tasks are pushed onto a single `FuturesUnordered` stream and consumed as they complete. The evaluator still collects all in-flight results and aggregates errors by stable term index, preserving consensus behavior while removing the phase barrier.
2. **No `ChargingRSpace` wrapper.** Each sub-term future calls `ISpace::produce` / `ISpace::consume` directly via the raw `RhoISpace = Arc<Box<dyn ISpace<…> + Send + Sync>>` alias. No per-application-COMM `handle_result` bookkeeping is required because cost is consumed by source-token matches, not storage-cost wrappers.
3. **`CostManager` replaced by `TokenBudget`.** Instead of intercepting every produce/consume, source tokens are consumed through recursive metered source events (Section 5.3); the cost layer becomes a compact atomic counter plus deterministic event metadata.
4. **Deterministic error handling.** Errors do not short-circuit in completion order. They are collected and reported in a stable order so parallel scheduling cannot change the consensus-visible failure.

### 5.5 Split/Join Deployment

The `Split` and `Join` mediator processes for compound signatures are persistent infrastructure contracts deployed once at chain genesis and available to all subsequent deploys. They are not per-deploy artifacts; they are part of the system's trusted computing base.

For each compound signature `s₁ & s₂` that appears in the system:

```
Split(s₁, s₂) = for(t ← N⟦s₁ & s₂⟧) ( N⟦s₁⟧!(0) ∣ N⟦s₂⟧!(*t) )
Join(s₁, s₂)  = for(t₁ ← N⟦s₁⟧) for(t₂ ← N⟦s₂⟧) ( N⟦s₁ & s₂⟧!(*t₁ ∣ *t₂) )
```

These are deployed with the `install` method of `ISpace`, which registers a persistent pattern-continuation pair without consuming phlogiston. They fire automatically whenever a compound token needs to be decomposed (Split) or two atomic tokens need to be recombined (Join).

The Rocq formalization supports persistence through a **two-lens design**:

1. The `PReplicate` constructor (`PReplicate : proc → proc`) with reduction rule `PReplicate P ⇝ P ∣ PReplicate P` is the primitive view, matching Rholang's `contract x(y) = { P }` runtime semantics (compiled to `Receive { persistent := true }`). The persistent mediators are defined as `PersistentSplit(s₁, s₂) = PReplicate(Split(s₁, s₂))` and `PersistentJoin(s₁, s₂) = PReplicate(Join(s₁, s₂))`, with closedness proven in `Translation.v`.

2. Meredith's reflective replication encoding from [1, §3] is mechanized in `theories/Replication.v`: `D_encoding x ≜ for(y ← x){x⟨|*y|⟩ ∣ *y}` and `bang_encoding x P ≜ x⟨|D(x) ∣ P|⟩ ∣ D(x)`. The operational fact `bang_encoding_unfolds` proves that one COMM step of the encoding produces `bang_encoding x P ∣ P` — exactly matching `PReplicate P → P ∣ PReplicate P`. This satisfies the paper's §5 Remark claim that the encoding "applies directly".

Together: 23 modules, 18,550 lines, 624 `Qed`/`Defined` proof terms, zero admissions, and zero axioms.

In practice, most deploys use atomic signatures (`Sig::Hash`), so Split/Join are only needed for multi-party authorization scenarios (e.g., multi-sig wallets, joint accounts).

### 5.6 Component Diagram: New Architecture

```
  ┌─────────────────────────────────────────────────────────────────────┐
  │                        Deploy Pipeline                              │
  │                                                                     │
  │  ┌────────────────┐    ┌────────────────┐    ┌────────────────────┐ │
  │  │   Parser +     ├───▶│ Signature      ├───▶│ Recursive Metering │ │
  │  │   Normalizer   │    │ Annotator      │    │ Pass               │ │
  │  └──────┬─────────┘    └───────┬────────┘    └─────────┬──────────┘ │
  │         │                      │                       │            │
  │    Plain Par           SignedProcess            Metered Work Queue  │
  │                                                        │            │
  └────────────────────────────────────────────────────────┊────────────┘
                                                           │
                                                           ▼
  ┌─────────────────────────────────────────────────────────────────────┐
  │                        Evaluator (reduce.rs)                        │
  │                                                                     │
  │  ┌────────────────────┐                                             │
  │  │ DebruijnInterpreter│                                             │
  │  │                    │                                             │
  │  │  eval_inner()      │     ┌───────────────────┐                   │
  │  │  FuturesUnordered  │◀───▶│  RSpace (ISpace)  │                   │
  │  │  (no Phase 1/2)    │     │ raw + metered hook│                   │
  │  │                    │     └────────┬──────────┘                   │
  │  └──────┬─────────────┘              │                              │
  │         │                            │                              │
  │         ▼                            ▼                              │
  │  ┌────────────────────┐     ┌───────────────────┐                   │
  │  │ TokenBudget        │     │ LMDB Backing      │                   │
  │  │ (weighted tokens)  │     │ Store             │                   │
  │  └────────────────────┘     └───────────────────┘                   │
  │                                                                     │
  └─────────────────┬───────────────────────────────────────────────────┘
                    │
                    ▼
  ┌─────────────────────────────────────────────────────────────────────┐
  │                    Persistent Infrastructure                        │
  │                                                                     │
  │  ┌──────────────────┐  ┌──────────────────┐  ┌───────────────────┐  │
  │  │ Split(s₁, s₂)    │  │ Join(s₁, s₂)     │  │ (installed once   │  │
  │  │ mediator procs   │  │ mediator procs   │  │  at genesis)      │  │
  │  └──────────────────┘  └──────────────────┘  └───────────────────┘  │
  │                                                                     │
  └─────────────────────────────────────────────────────────────────────┘
```

Key architectural changes visible in the diagram:

1. **Two new pipeline stages** (Signature Annotator, Recursive Metering Pass) between the parser and the evaluator.
2. **No broad ChargingRSpace wrapper.** The evaluator uses the raw RSpace path plus a narrow internal metered-gate match hook for source-token accounting.
3. **TokenBudget** replaces CostManager for user deploy consensus charging. It uses a single atomic counter for billable source-token events plus deterministic event metadata.
4. **`FuturesUnordered`** in the eval loop replaces f1r3node-rust's sequential `JoinHandle`-await loop. Branch tasks are drained by a single `FuturesUnordered` driver; in-flight errors are still aggregated in stable order.
5. **Persistent infrastructure** (Split/Join mediators) installed at genesis.

### 5.7 Structural Equivalence and RSpace Correspondence

The Rocq formalization includes `rs_struct` (closure of reduction under structural equivalence `≡`), which allows reduction to proceed modulo commutativity and associativity of parallel composition. The f1r3node RSpace implementation handles this equivalence implicitly through normalization and pattern matching. This subsection documents the correspondence and its verification requirements.

**Channel normalization.** RSpace stores data and continuations keyed by Blake2b256 hashes of serialized channel terms. Structurally equivalent channels (e.g., `@(P ∣ Q)` and `@(Q ∣ P)`) must hash to the same key for the fuel-gate protocol to function correctly — otherwise a token output on `N⟦s₁ & s₂⟧ = @(*N⟦s₁⟧ ∣ *N⟦s₂⟧)` could land in a different hash bucket than a fuel-gate input on the "same" channel with its parallel components in a different order. The Rholang normalizer canonicalizes parallel compositions by sorting sub-terms by their Blake2b256 hashes before serialization, ensuring that `≡`-equivalent channels map to the same RSpace key. This normalization is performed in the `Normalizer` pass (`rholang/src/rust/interpreter/compiler/normalize.rs`) before terms enter the tuple space.

**Pattern matching.** The `SpatialMatcher` in `rholang/src/rust/interpreter/matcher/` handles commutativity and associativity of parallel composition during pattern matching. When a fuel-gate input `for(t ← N⟦s⟧)(…)` is matched against a token output `N⟦s⟧!(payload)`, the matcher must recognize that the channel expressions are equivalent even if their internal parallel sub-terms appear in different orders. The `SpatialMatcher` provides this by exhaustively searching over permutations of parallel components, providing the runtime equivalent of the Rocq `rs_struct` rule.

**Verification requirement.** Migration step 18 (Section 6) should include tests verifying that structurally equivalent deploys produce identical costs under the internalized model. Specifically:

- Deploys whose bodies differ only by associativity of parallel composition (e.g., `(x | y) | z` vs `x | (y | z)`) must produce the same `TokenBudget.total_cost()`.
- Deploys whose bodies differ only by commutativity of parallel composition (e.g., `x | y` vs `y | x`) must produce the same cost.
- Deploys whose signature channels involve compound signatures (e.g., `N⟦s₁ & s₂⟧` vs `N⟦s₂ & s₁⟧`) must produce the same cost, verifying that the normalizer canonicalizes the parallel composition in `@(*N⟦s₁⟧ ∣ *N⟦s₂⟧)` consistently.

Any divergence between these equivalence classes indicates a gap between RSpace's normalization and the Rocq structural equivalence relation, which would undermine the cost determinism guarantee.

The Rocq development states the theorem modulo `≡`; RSpace's concrete
normalizer is Rust implementation territory. The migration therefore
discharges the correspondence behaviorally in the Rust suite:
structurally equivalent deploys must produce identical token costs, and
compound signature channels must be canonicalized before use as fuel
channels. The concrete checks cover the relevant `≡` axioms against the
live normalizer:

- **Identity** (`P ∣ 0 ≡ P`): for randomized `P`, assert `normalize(par(P, Nil)) == normalize(P)`.
- **Commutativity** (`P ∣ Q ≡ Q ∣ P`): for randomized `P, Q`, assert `normalize(par(P, Q)) == normalize(par(Q, P))`.
- **Associativity** (`(P ∣ Q) ∣ R ≡ P ∣ (Q ∣ R)`): for randomized `P, Q, R`, assert `normalize(par(par(P, Q), R)) == normalize(par(P, par(Q, R)))`.
- **Alpha-renaming**: for randomized `P`, rename bound names to fresh ones and assert the normalized forms agree (modulo the binder's canonical representation).

These tests are a **consensus-critical validation step** — a divergence at runtime would not invalidate the Rocq proofs (which are stated modulo `≡`) but would break cost determinism in the deployed system. Any failing property is a bug in the normalizer, not a question of design intent, and must be fixed in the normalizer rather than worked around elsewhere. The verification doc §12.3 records this as an implementation boundary rather than as an unclaimed formal theorem.

### 5.8 Fuel Residue Management

The formal calculus treats unconsumed tokens and stuck residues as inert terms in a parallel composition — the bisimulation proof (`multi_stuck_residue_bisim` in `Bisimulation.v`) shows that `P ∣ *(@0)` is observationally equivalent to `P`, so residues are semantically invisible. However, the physical RSpace storage layer cannot ignore them: every unconsumed token output and every stuck residue occupies real memory in the HotStore and disk space in LMDB. Without explicit residue management, these artifacts accumulate across deploys and degrade storage performance. This subsection specifies how the implementation prevents, cleans up, and accounts for fuel-gate residues.

#### 5.8.1 Per-Deploy Signature Scoping

The Signature Annotator (Section 5.2) derives the deploy's signature from the deploy's cryptographic signature field. If the signature channel were derived solely from the deployer's public key (`Sig::Hash(deployer_public_key_hash)`), then signature channels would be **per-deployer, not per-deploy**: unconsumed tokens from deploy A would sit on the same channel as tokens from deploy B (same deployer), and a later deploy could consume leftover tokens from an earlier deploy — effectively receiving "free" fuel that was never allocated to it.

To prevent cross-deploy token leakage, the Signature Annotator uses a domain-separated digest of the deploy's cryptographic signature (`deploy.sig`), which is unique per deploy (it signs `hash(term) + timestamp` with the deployer's private key):

```
FUNCTION deploy_signature(deploy: Signed<DeployData>) -> Sig:
    domain = "f1r3node:cost-accounted-rho:deploy-signature:v1"
    RETURN Sig::Hash(blake2b256(domain || deploy.sig))
```

This produces a unique signature channel `N⟦SHash(blake2b256(domain || deploy.sig))⟧` for each deploy. Because each deploy has a unique cryptographic signature (distinct term content or timestamp produces a distinct signature under any secure signing scheme), and `blake2b256` preserves distinctness (collision-resistant), the resulting signature channels are deploy-isolated.

**Formal support for channel isolation.** The syntactic disjointness of deploy channels follows from three results in the Rocq mechanization, which compose to guarantee that distinct deploys cannot share a fuel-gate channel: (1) `hash_process_injective` (verification doc §12.1, hypothesis #2) — collision resistance of the underlying hash inherits to `hash_process`; (2) `N_tr_is_Quote` (`ChannelSeparation.v:115`) — every signature-derived channel is a `Quote` of a structured process; and (3) `fuel_gate_no_app_channel_overlap` (`ChannelSeparation.v:179`) — fuel-gate channels do not alias with ordinary application channels. Composing these: for deploys with `deploy1.sig ≠ deploy2.sig`, `blake2b256(domain || deploy1.sig) ≠ blake2b256(domain || deploy2.sig)` (by collision resistance), `hash_process(b1) ≠ hash_process(b2)` (by hypothesis #2), `N_tr (SHash b1) ≠ N_tr (SHash b2)` (by `N_tr`'s structural injectivity on hash signatures), so the fuel-gate channels are syntactically distinct and tokens cannot leak across deploy boundaries.

**Compatibility with the Rocq formalization.** The `hash_process_injective` hypothesis requires that the hash function be injective. Distinct deploys have distinct domain-separated `deploy.sig` inputs (by the unforgeability and collision resistance of the signing scheme), so `blake2b256` of those inputs produces distinct hashes with overwhelming probability. The `hash_process_closed` and `hash_process_head_count_one` hypotheses are unaffected, since the output construction (`PDeref(Quote(GUnforgeable(GPrivate(...))))`) is the same regardless of what bytes are hashed.

**Peek bypass prevention.** Signature channels use `GPrivate` unforgeable names derived from the domain-separated deploy-signature hash. User code running inside the fuel-gate body cannot forge or reference these channels — the signature channel name is in the fuel-gate prefix, not exposed to the body `P`. This is formally proven by `fuel_gate_stuck_isolated` in `FuelGateSafety.v`: the body `P` cannot communicate on the fuel-gate channel. Consequently, peek operations on signature channels are impossible from user code, and the concern that peeks could bypass fuel consumption does not arise.

#### 5.8.2 Post-Evaluation Token Sweep

After a deploy's evaluation completes successfully, internal fuel artifacts may remain on the deploy's signature channel(s): authorization markers, non-billable Split/Join routing markers, or unfired continuation-keyed gates. The formal nested-stack model represents unused fuel as token outputs, but the production runtime coalesces fuel in `TokenBudget` and must not materialize one tuple-space object per phlo.

The evaluator performs a **token sweep** after evaluation completes and before the hard checkpoint is taken. The sweep removes fuel artifacts (data) and unfired fuel-gate inputs (continuations) from the deploy's signature channels. Unfired gates arise when a process terminates early or an already-discovered successor never becomes billable:

```
FUNCTION sweep_unconsumed_tokens(space: &mut ISpace, deploy_sig: Sig):
    LET channel = SignatureChannel::from_sig(&deploy_sig).par
    // Remove all internal fuel data on the deploy's signature channel.
    space.remove_all_data(&channel)
    // Remove all unfired fuel-gate continuations on the deploy's signature channel.
    space.remove_all_continuations(&channel)

    // For compound signatures, also sweep the component channels.
    IF deploy_sig IS Sig::And(s1, s2):
        sweep_unconsumed_tokens(space, *s1)
        sweep_unconsumed_tokens(space, *s2)
```

**New `ISpace` methods.** The `remove_all_data` and `remove_all_continuations` methods are new additions to the `ISpace` trait. They delegate to the existing per-index `remove_datum` and `remove_continuation` methods in `HotStore` (`rspace++/src/rspace/hot_store.rs`), iterating over all indices for the given channel.

The sweep is a deterministic operation: all validators perform the same sweep on the same channel(s), removing the same set of internal fuel artifacts and unfired gates, yielding the same final tuple-space state. Because signature channels are per-deploy (Section 5.8.1), the sweep cannot interfere with other deploys' tokens or fuel gates.

**Formal support for sweep determinism.** The sweep's determinism follows from `fuel_events_consumed_perm` (`FuelEventDecomposition.v:198`, Theorem 9.18 in verification doc §9.7): the multiset of consumed fuel events is determined solely by the start and end states of the reduction, independent of scheduling order. The residual logical fuel (= initial token units minus consumed units) is therefore also schedule-independent. In the bounded runtime, that residual lives in `TokenBudget.remaining()` rather than as one physical output per unit; the sweep targets only the deterministic physical artifacts left by the metering protocol. This complements `ca_cost_deterministic` (consumed count is fixed) with a deterministic cleanup rule.

**Cost accounting interaction.** The `TokenBudget.total_cost()` returns the number of billable source tokens consumed, not the number of tokens remaining and not the number of routing COMMs. The sweep removes the physical residues without affecting the cost counter. The final cost reported to consensus is `TokenBudget.total_cost()`, which is authoritative. The sweep is purely a storage optimization — it does not change the semantics of the cost computation.

**Failed deploys.** When a deploy fails (e.g., `OutOfPhlogistonsError`), the soft checkpoint revert (`revertToSoftCheckpoint`) already discards all tuple-space changes made during the deploy, including internal fuel artifacts and fuel-gate entries. No additional sweep is needed for failed deploys.

#### 5.8.3 Stuck Residue Elision

Each fuel-gate COMM firing produces a stuck residue: when the fuel gate `for(t ← N⟦s⟧){ lift(P, 1, 0) ∣ *t }` fires with token payload `@(T⟦t'⟧)`, the body becomes `P ∣ T⟦t'⟧`. The innermost token in the stack is always `T⟦TUnit⟧ = PNil`, which produces a dereference chain terminating in `*(@0)` — a `PDeref(Quote(PNil))` term. By the quote-dereference cancellation law (`*(@P) ≡ P` for closed `P`), `*(@0) ≡ 0`, so these residues are structurally equivalent to nil.

Rather than allowing stuck residues to enter the tuple space and cleaning them up later, the evaluator **elides** them at evaluation time:

```
FUNCTION eval_term(term: &Par, env: &Env, rand: Blake2b512Random) -> Result<()>:
    // Recognize stuck residues and skip evaluation.
    IF is_stuck_residue(term):
        RETURN Ok(())
    // ... normal evaluation logic ...

FUNCTION is_stuck_residue(term: &Par) -> bool:
    // PDeref(Quote(PNil)) — the canonical stuck residue.
    MATCH term:
        PDeref(Quote(PNil)) => true
        // PPar of stuck residues is also a stuck residue
        // (compound gates produce PPar(*(@0), *(@(*(@0)))))
        PPar(a, b) => is_stuck_residue(a) && is_stuck_residue(b)
        PNil => true
        _ => false
```

This optimization is sound because:
1. `PDeref(Quote(PNil))` is structurally equivalent to `PNil` by the quote-dereference cancellation law.
2. `PNil` is the stopped process — it performs no computation and produces no tuple-space entries.
3. The bisimulation proof (`multi_stuck_residue_bisim` in `Bisimulation.v`) confirms that `P ∣ *(@0)` is observationally equivalent to `P`.

**Formal support for observational inertness.** The soundness of eliding `*(@0)` residues rests on three Rocq results cited in verification doc §12.3: `deref_no_barb` (a `PDeref` cannot exhibit a top-level barb/I/O action), `backward_sim_par_stuck` (a parallel composition with a stuck residue cannot originate any COMM that the residue participates in), and `post_gate_bisim` (the post-gate residue is strongly bisimilar to `Nil`). Together these guarantee that eliding the residue preserves all observable behaviour, so all validators that apply the elision converge on the same post-evaluation state.

Stuck residue elision prevents storage accumulation at the source rather than requiring post-hoc cleanup.

#### 5.8.4 Split/Join Mediator Lifecycle

Section 5.5 specifies that Split and Join mediators are deployed at chain genesis as persistent infrastructure. In practice, most deploys use atomic signatures (`Sig::Hash`), so Split/Join mediators are only needed for multi-party authorization scenarios (e.g., multi-sig wallets, joint accounts).

**On-demand deployment.** Not all compound signatures can be anticipated at genesis. When the Signature Annotator encounters a compound signature `SAnd(s₁, s₂)` for which no Split/Join mediators are installed, it deploys them on demand:

```
FUNCTION ensure_mediators(space: &mut ISpace, s1: &Sig, s2: &Sig):
    LET compound_chan = SignatureChannel::from_sig(&Sig::And(s1.clone(), s2.clone()))
    // Check if a persistent continuation is already registered on the compound channel.
    IF NOT space.has_persistent_continuation(&compound_chan.par):
        // Deploy persistent Split mediator.
        space.install(
            vec![compound_chan.par.clone()],
            vec![wildcard_pattern()],
            split_continuation(s1, s2),
        )
        // Deploy persistent Join mediator.
        space.install(
            vec![SignatureChannel::from_sig(s1).par,
                 SignatureChannel::from_sig(s2).par],
            vec![wildcard_pattern(), wildcard_pattern()],
            join_continuation(s1, s2),
        )
```

The `install` method registers persistent pattern-continuation pairs without consuming phlogiston, consistent with the existing system deploy mechanism used for built-in Rholang system contracts.

**New `ISpace` method.** The `has_persistent_continuation` method is a new addition to the `ISpace` trait. It is implemented as `get_waiting_continuations(vec![channel]).iter().any(|wc| wc.persist)`, using the existing `get_waiting_continuations` method from `rspace_interface.rs`.

**Storage overhead.** Persistent mediators are registered once per compound signature and stored as `WaitingContinuation` entries with `persist: true`. Each mediator occupies a fixed amount of storage (one continuation entry with patterns). Because compound signatures are relatively rare (most deploys use atomic signatures), the total storage overhead for mediators is negligible compared to application data. No garbage collection of mediators is required — they remain available for future deploys that use the same compound signature.

#### 5.8.5 Logical vs. Physical Token Accounting

The `TokenBudget` (Section 5.3) maintains a **logical** cost counter via `AtomicU64`. The **physical** tuple-space state contains internal authorization markers, routing markers, and unfired gate continuations on signature channels. These two views may temporarily diverge during evaluation:

- A billable source-token match fires, incrementing the logical counter, but the async runtime has not yet cleaned up all internal routing artifacts.
- Stuck residues are elided (Section 5.8.3) before entering the tuple space, so the physical state has fewer terms than the logical model would suggest.

**Authoritative source for consensus.** The `TokenBudget.total_cost()` (the logical counter) is authoritative for consensus. It represents the number of billable source-token units reserved during evaluation, which is the definition of cost in the internalized model. The physical tuple-space state after the token sweep (Section 5.8.2) is a clean storage artifact — it contains no fuel-gate residues and no metering artifacts on the deploy's signature channels. Validators compare the logical cost counter for consensus, not the physical tuple-space state of signature channels.

**Invariant.** After evaluation and the token sweep, the following invariant holds:

```
TokenBudget.total_cost() + TokenBudget.remaining() = initial_tokens
```

where `initial_tokens` is the deploy's phlogiston limit. This invariant can be checked as a debug assertion during migration testing (Section 6, steps 13–14) to verify that no billable source-token units are created or destroyed during evaluation — they are only consumed or left as logical remaining budget. The separate sweep assertion is that no metering artifacts remain in RSpace on the deploy's signature channels. Non-billable Split/Join routing markers must be tracked separately and must not affect the cost equality.

#### 5.8.6 Partial-Execution and Out-of-Phlogiston Semantics

A deploy that exhausts its phlogiston budget mid-evaluation raises `OutOfPhlogistonsError`. By that point the evaluator may have:

- consumed some prefix of the deploy's billable source-token sequence (incrementing `TokenBudget.total_cost()`),
- spawned `Split` mediator firings that decomposed compound authorization markers into atomic routing markers,
- spawned `Join` mediator firings that combined atomic routing markers into compound markers,
- written application-level state (sends, consumes, COMM bodies) into the tuple space.

**Disposition.** F1R3Node uses the existing `SoftCheckpoint` mechanism (`rspace++/src/rspace/checkpoint.rs`) to roll back *all* tuple-space state changes performed by the failed deploy, including:

- All internal fuel artifacts (authorization markers, routing markers, unfired gates) on the deploy's signature channels.
- All application-level produces, consumes, and COMM events.
- All stuck residues (whether elided or persisted).

The soft-checkpoint revert is unconditional and uniform: the post-revert tuple-space state is bit-identical to the pre-deploy state, modulo the deterministic event-log entries that record the failed execution and the sweep operations.

**Cost retention (no refund).** Phlogiston consumed by billable source-token firings *before* the OOP boundary is *not* refunded. The `TokenBudget.total_cost()` at the moment of failure is the cost the deployer pays. This matches the existing F1R3Node semantics (`OutOfPhlogistonsError` already burns charges from the externalized model) and avoids the re-entrancy and accounting complexity that a refund mechanism would introduce. Deployers who wish to bound risk do so by sizing their phlo limit appropriately, exactly as today.

**Determinism across validators.** The OOP point is a deterministic function of the deploy's execution: every validator firing the same billable source-token sequence in the same canonical order (Section 8, "Event hashing") reaches the same `TokenBudget.total_cost()` and therefore raises `OutOfPhlogistonsError` on the same billable source-token firing. The event log records the OOP point as part of the deterministic per-deploy event stream, replayable via rig-and-reset like any other event sequence.

**Cost-side proof support.** `token_strictly_decreases` (`TokenConservation.v:226`) guarantees that every `ca_step` consumes a strictly positive amount of fuel; therefore the OOP boundary, being a function of the cumulative consumed fuel, is well-defined as the smallest `n` for which the `n`-th `ca_step`'s fuel demand exceeds the remaining budget. The `ca_max_steps_bound` corollary (`StrongNormalization.v:111`) puts an absolute bound on how many steps a deploy can take before the calculus forces termination, which guards the OOP detection loop against pathological inputs.

### 5.9 Complete Replacement Surface in This Repository

The staged implementation in this repository shows that replacing the retired
`ChargingRSpace` role alone is not a complete migration. Cost-accounted rho
touches the interpreter, substitution, primitive operations, FFI, Casper fee
settlement, replay validation, metrics, API documentation, and tests. The
current replacement surface is:

| Rust surface | Current role |
|--------------|--------------|
| `rholang/src/rust/interpreter/accounting/mod.rs` | `RuntimeBudget`, `BillableTokenEvent`, deterministic source-event weights, bounded trace retention, and replay-authenticated digest/count state. |
| `rholang/src/rust/interpreter/metering.rs` | `MeteredMachine` bridge from interpreter frames to billable/nonbillable/system runtime-budget operations. |
| `rholang/src/rust/interpreter/reduce.rs` | `FuturesUnordered` branch dispatch, stable error aggregation, and recursive metering of billable source events. |
| `rholang/src/rust/interpreter/rho_runtime.rs` | Runtime construction with raw RSpace access and `RuntimeBudget`/unmetered budget selection by execution mode. |
| `rholang/src/lib.rs` and `rholang/src/rholang_cli.rs` | Public cost reporting remains shape-compatible while `value` is interpreted as consumed source-token units and operation labels stay diagnostic/non-consensus. |
| `casper/src/rust/rholang/runtime.rs` | Feeds deploy phlo limit into runtime-budget evaluation, records token cost in `ProcessedDeploy.cost`, and computes post-evaluation fee settlement from token cost. |
| `casper/src/rust/rholang/replay_runtime.rs` | Replays the metered path and compares replayed cost trace digest/count/status against processed deploy evidence. |
| `casper/src/rust/util/rholang/runtime_manager.rs` | Includes canonical cost trace digest/count and cost-bearing deploy fields in replay payload hashing/cache keys without depending on wall-clock completion order. |
| `models/src/rust/casper/protocol/casper_message.rs` | `ProcessedDeploy::refund_amount()` uses `phlo_limit - cost` | Keep formula shape but define `cost` as token units. Guard overflow and negative values explicitly. |
| `casper/src/rust/validate.rs`, APIs, websocket events, docs | Phlo-price validation and user-visible cost fields | Keep `phlo_price >= min_phlo_price`; update descriptions so `cost` means consumed source-token units. |
| Test suites under `rholang/tests`, `casper/tests`, API docs | Hard-coded old cost totals and old determinism assumptions | Replace expected costs with token-model fixtures; add replay, OOP, settlement, and high-parallelism regression tests. |

Post-activation replay must treat the cost-trace digest and event count as
required consensus evidence. A cost-accounted processed deploy without a
cost-trace digest is replay-invalid, even if the scalar cost matches,
because scalar cost alone does not authenticate the sequence of billable
source-token events that justified fee settlement. Legacy non-cost-accounted
replay remains accepted only through the explicit compatibility path. A
zero-event deploy is not a special exemption: it carries a present digest
commitment over an empty event set and an event count of zero, so the
consensus distinction is "commitment present" rather than "trace non-empty."

The trace boundary is also part of the replacement surface. Failed deploy
rollback reverts tuple-space effects but retains the OOP boundary evidence
needed for replay, oversized runtime weights are rejected before mutating
cost or trace state, and scheduler/control frames remain non-billable and
therefore cannot enter the consensus cost trace.

#### 5.9.1 Primitive Work and Parser/Normalizer Costs

The source calculus proves correctness for Rules 1-5. The Rust
interpreter also charges work that sits outside that small calculus:
parsing, normalization, substitution, pattern matching, arithmetic,
collections, string/bytes operations, BigInt/BigRat operations, and
pathmap operations. A complete replacement must route each such cost
through one of two explicit paths:

1. **Billable source-token event.** If the work is consensus-billable,
   define a deterministic `BillableTokenEvent` descriptor and weight.
   The descriptor must be derived from normalized source position,
   operation kind, and consensus-visible operand sizes. The event is
   reserved through `TokenBudget::reserve_canonical`, never through a
   legacy `CostManager::charge` call.

2. **Admission/resource limit outside consensus cost.** If the work is
   only a denial-of-service guard before a deploy reaches consensus
   evaluation, document it as an admission limit with deterministic
   rejection semantics. It must not be mixed into `ProcessedDeploy.cost`.

Weighted primitive events are a finite coalescing of unit source-token
steps: a weight `w` event is equivalent at the cost boundary to `w`
unit token consumptions with the same canonical descriptor prefix. This
keeps the bounded-memory runtime aligned with token conservation while
avoiding an impractical expansion into `w` physical fuel gates. The
implementation must add tests that compare weighted reservations with
the equivalent unit-event expansion for small `w`, and must reject any
weight computation that depends on heap layout, task completion order,
hash-map iteration order, or host-specific numeric behavior.

Parsing and normalization need special handling because they happen
before the metered source state exists. The safe design is:

- malformed source is rejected before consensus evaluation and reports
  zero consumed token cost;
- normalization is deterministic and covered by fixed admission resource
  limits;
- any future decision to bill normalization must introduce a synthetic
  source-token event whose descriptor is a hash of the normalized input,
  not a sequence of host-local parser callbacks.

#### 5.9.2 Casper Fee Settlement Is Not Runtime Metering

The `costacc` system deploys and Casper precharge/refund code remain
part of fee settlement, not the runtime cost-accounting algorithm. The
new runtime computes `token_cost = TokenBudget.total_cost()` for the
user deploy. Casper then settles payment using the existing economic
shape:

```
escrowed_amount = phlo_limit * phlo_price
charged_amount  = token_cost  * phlo_price
refund_amount   = escrowed_amount - charged_amount
```

This is a two-ledger boundary. During user evaluation the only mutable
cost state is the deploy-local `TokenBudget`, and that budget is
monotone: it may reserve source-token units or fail at a canonical OOP
descriptor, but it may not receive refunds, top-ups, balance transfers,
or a copied continuation with a larger remaining balance. Casper balance
movement happens only after the deploy has either produced a final
token count or failed with its deterministic OOP count. A deploy that
needs to observe a refunded purse balance must do so in a later deploy
or continuation after the system settlement deploy has committed.

The fee-settlement arithmetic is deliberately small enough to audit:
for a valid deploy with non-negative `phlo_limit` and `phlo_price`, and
with `token_cost <= phlo_limit`, the refund is bounded by the escrow and
`charged_amount + refund_amount = escrowed_amount`. Runtime validation
must reject negative limits or prices before precharge, and the PoS
refund contract must reject refunds larger than the deploy's recorded
initial payment. These checks make the Rholang system deploy a bounded
ledger operation over the runtime's consumed-token count rather than a
second path for changing evaluation fuel.

Signatures and traces sit on the same boundary. The deploy signature
authenticates the deploy data that includes `phlo_limit`, `phlo_price`,
`term`, timestamp, shard, and expiry. The proposer block signature
authenticates the block hash, which includes every `ProcessedDeploy`,
its consumed-token `cost`, cost-trace digest, cost-trace event count,
and the deploy event log used by replay. Replay-cache keys must include
the same replay payload so optimization cannot reuse a cached result
across digest, count, failure-status, user-log, system-log, slash-field,
or genesis-mode changes.
The evaluation trace is therefore useful for assurance and deterministic
replay: it records the sequence of tuple-space effects and tie-breaker
choices that led to the final state and cost. It does not authorize
Casper to mutate balances during evaluation; it authenticates the
post-evaluation settlement input.

The settlement deploys themselves run as system deploys under an
explicit unmetered/no-op budget. They must not preserve the old
`CostManager` as a second runtime metering path. This keeps the trust
boundary clean: user-code execution is cost-accounted by recursive
metering; privileged fee movement is deterministic system accounting
over the resulting token count.

Slashing composes with this boundary rather than entering the runtime
metering algorithm. The slashing protocol's formal source remains
f1r3node-rust's `analysis/slashing` branch, which proves slash
authorization, slash effect correctness, two-level closure, validator
lifetime handling, and the Rust/Scala slashing bisimilarity. This
cost-accounting branch adopts only the interface needed for fee
settlement: a slash system deploy may update PoS bond, vault, active-set,
and slashed-validator state, but it must preserve the user deploy's
`phlo_limit`, `phlo_price`, computed `token_cost`, final user fuel, and
escrow/refund arithmetic. The bridge now models the slashing-side
current-evidence predicate explicitly: recovered rejected slashes require
both evidence epoch and target activation epoch to equal the current
epoch, authorization reads the parent pre-state bond rather than an
ambient post-state view, and a zero-bond no-op slash preserves the cost
boundary. Mechanized bridge theorems in `SlashingComposition.v` prove
that current cost-invalid block evidence can feed the slashing evidence
pipeline without changing the already-computed user cost, and that
applying a slash effect after evaluation cannot add fuel or alter
settlement.

`ProcessedDeploy.cost.cost` remains the consensus-visible numeric field,
but its unit changes to consumed source-token units. `CostProto.operation`
and old operation-labelled logs, if retained for compatibility, are
diagnostic only and must not affect replay validation, block hashes, or
fee settlement.

#### 5.9.3 Parallelism and Canonical OOP Boundaries

The design maximizes parallelism by separating discovery from budget
commit:

1. The normalized source tree assigns every potential source redex a
   stable `SourcePath` and redex id.
2. Reducer workers discover enabled redexes concurrently and compute
   their `BillableTokenEvent` descriptors and weights without touching a
   global cost mutex.
3. The metering kernel commits the ready billable events in canonical
   descriptor order for the deploy, calling
   `RuntimeBudget::commit_canonical_batch`.
4. Successfully permitted continuations resume immediately and may spawn
   further metered work through `FuturesUnordered`; permit grant consumes
   phlo and is not refunded by later state rollback.
5. If the next canonical reservation would exceed the budget, every
   validator raises `OutOfPhlogistonsError` at the same descriptor.

The canonical ordering is therefore a consensus boundary for insufficient
fuel, not a sequential execution plan. RSpace matching, continuation
execution, primitive work, and branch discovery remain concurrent. Event
logs and replay payloads must use canonical descriptors or multiset
commitments for fuel events; they must not depend on wall-clock task
completion order.

The security boundary is physical work, not only surviving state. A branch
may discover cheap descriptors, but expensive primitive work, substitution,
RSpace search, hashing, serialization, continuation execution, and spawn
must be preceded by a charged execution permit or a deterministic admission
cap. User-visible effects can roll back after OOP; the permit charge remains.

#### 5.9.4 Acceptance Criteria for Complete Replacement

The `f1r3node-rust` implementation is not complete until all of the
following are true:

- `rg "CostManager::charge|cost\\.charge|ChargingRSpace"` finds no
  user-deploy runtime path; any remaining occurrences are tests,
  compatibility shims, historical docs, or explicitly unmetered system
  infrastructure.
- Every primitive-operation and substitution charge site is represented
  as a deterministic weighted source-token event or as a documented
  non-consensus admission limit.
- Replay validation recomputes `TokenBudget.total_cost()` and rejects a
  `ProcessedDeploy.cost.cost` mismatch.
- Fee settlement uses `token_cost * phlo_price` and `phlo_limit -
  token_cost` with explicit overflow/underflow guards.
- High-parallelism tests force different task completion orders while
  producing identical token cost, canonical OOP descriptor, replay
  digest, and final tuple-space state.
- Benchmarks show the eval loop uses `FuturesUnordered` or equivalent
  bounded fan-out polling, with no global cost mutex on the hot path.

---

## 6. Migration Strategy

The migration from the externalized cost model to the internalized model is a forward-only replacement. The baseline externalized cost model is correct (Section 3.1) but suboptimal along three structural dimensions (Section 3.2): no machine-checked formal proof of cost determinism, intricate per-COMM refund bookkeeping in `handle_result`, and no first-class composable metering primitives. The internalized model upgrades each of these. Because the two cost models report numerically different totals, there is no dual-mode comparison and no in-place rollback path — activation is coordinated at a consensus boundary (step 22). The steps below are ordered topologically: each step depends only on steps above it.

**Mapping to paper §6.4 phases.** The steps below realize the four-phase implementation path sketched in [4, §6.4]:

| Paper phase                                           | Migration steps                                                          |
|-------------------------------------------------------|--------------------------------------------------------------------------|
| **Phase 1** — translation as compiler pass replacing runtime hooks | Steps 1–7 (IR types, signature/token functions, and recursive metering kernel), 15 (pipeline integration), 17 (retire `ChargingRSpace` and legacy `CostManager::charge` from the user path) |
| **Phase 2** — deploy persistent splitters/joiners and infrastructure processes | Step 11 (`ensure_mediators` on-demand deployment), step 19 (test-network mediator install), and the genesis-style production deployment carried out at activation in step 22 |
| **Phase 3** — expose signature-channel and token APIs to user contracts | Out of scope for v1; the signature-channel infrastructure created by Phase 1 is sufficient for delegated and market-based user-space metering (Section 8.4) without additional runtime support |
| **Phase 4** — extend the formalization                | Discharged by `formal/rocq/cost_accounted_rho/` (23 modules, 18,550 lines, 624 `Qed`/`Defined` proof terms, zero admissions, zero axioms); paper §6.4 anticipated Lean 4, the present mechanization is in Rocq |

1. **Implement `Sig`, `Token`, `SignedProcess` types** (Section 5.1). These are the internal IR types that the translation pass operates on.

2. **Implement `SignatureChannel::from_sig`** — the `N⟦·⟧` translation (Section 4.1). Maps signatures to channel names.

3. **Implement `translate_token`** — the `T⟦·⟧` translation (Section 4.1). Maps tokens to parallel outputs on signature channels. Depends on step 2.

4. **Implement `recursive_metered_gate` and continuation keys** — the implementation counterpart of `recursive_metered_gate(K)` in `TranslationFaithfulness.v`. A gate authorizes exactly one selected source step and lands in the continuation for the source successor. Depends on step 2.

5. **Implement the recursive metering evaluator** — construct the `well_reflected` target incrementally: choose an enabled `ca_step`, install the continuation-keyed gate for that step, and re-enter the evaluator with the recursively metered successor. This is the implementation object covered by `well_reflected_backward_reflection`. Depends on steps 3 and 4.

6. **Implement per-deploy signature scoping** in the Signature Annotator: `Sig::Hash(blake2b256("f1r3node:cost-accounted-rho:deploy-signature:v1" || deploy.sig))` (Section 5.8.1). Depends on step 2.

7. **Implement the Signature Annotator.** Wraps a normalized `Par` in a `SignedProcess` using the deploy's cryptographic signature from deploy metadata (Section 5.2). Skips system deploys (`is_system_deploy_id`). Depends on steps 1 and 6.

8. **Implement `TokenBudget`** with atomic weighted reservations and canonical source-event descriptors (Section 5.3). No dependencies on the translation — it is a standalone budget primitive.

9. **Implement stuck residue elision:** `is_stuck_residue` recognition in the evaluator (Section 5.8.3). No dependencies on the translation — it is a pattern-recognition optimization in the evaluator.

10. **Implement the post-evaluation token sweep:** `sweep_unconsumed_tokens` (Section 5.8.2). Depends on step 2 (needs `SignatureChannel::from_sig` to identify which channels to sweep).

11. **Implement on-demand Split/Join mediator deployment:** `ensure_mediators` (Section 5.8.4). Depends on step 2.

12. **Add unit tests** for the signature/token functions and recursive metering kernel, cross-referenced with the Rocq proof expectations (`T_tr_unit`, `T_tr_gate`, `recursive_metered_gate_fires`, `recursive_metered_gate_per_step_reverse`, `well_reflected_backward_reflection`). Depends on steps 2–5.

13. **Add property-based tests** (proptest) verifying:
    - `translate_token(TUnit)` = `PNil`
    - A ready step in either side of `SPar(s1, s2)` can be selected without serializing the other side
    - For all `t`: `translate_token(t)` is closed (no free de Bruijn variables)
    - Per-deploy signature isolation: `deploy_signature(deploy1) ≠ deploy_signature(deploy2)` for deploys with distinct `sig` fields
    - Budget invariant: `total_cost + remaining = initial_tokens`
    - Token sweep removes all metering artifacts from deploy signature channels
    - Canonical source-event ordering selects the same OOP descriptor under randomized task completion orders
    - Weighted primitive events agree with equivalent unit-event expansion for small deterministic weights

    Depends on steps 2–6, 8, 10.

14. **Add residue management tests** verifying:
    - After each test deploy, the budget invariant holds: `TokenBudget.total_cost() + TokenBudget.remaining() = initial_tokens` (Section 5.8.5).
    - No metering artifacts remain on the deploy's signature channel(s) after the post-evaluation token sweep (Section 5.8.2).
    - Stuck residues (`PDeref(Quote(PNil))` and compositions thereof) are elided by the evaluator and do not appear as RSpace entries after evaluation (Section 5.8.3).
    - Deploys from the same deployer with different cryptographic signatures use distinct signature channels and cannot consume each other's tokens (Section 5.8.1).
    - Deploys that exhaust their phlogiston limit (`OutOfPhlogistonsError`) have all metering artifacts (authorization markers, routing markers, fuel-gate inputs, and stuck residues) discarded from the tuple space by the soft checkpoint revert.

    Depends on steps 8–10.

15. **Integrate the Signature Annotator and recursive metering evaluator into the deploy pipeline** (Section 5.2). Depends on steps 5 and 7.

16. **Replace f1r3node-rust's sequential `JoinHandle`-await eval loop with the `FuturesUnordered` eval loop** (Section 5.4). The new loop pushes all sub-term branch tasks onto a single `FuturesUnordered` driver, drops the `ChargingRSpace` wrapper (each sub-term future calls the raw `ISpace` directly), drains all in-flight futures, and aggregates errors in stable term order after every started branch has completed. Depends on steps 8 and 9 (the new eval path relies on `TokenBudget` and stuck-residue elision rather than `ChargingRSpace`).

17. **Remove the legacy charging framework from the user deploy path.** Remove `ChargingRSpace`, `storage_cost_produce`, `storage_cost_consume`, `handle_result`, and all `CostManager::charge` / `cost.charge` calls from user evaluation. Replace parser/normalizer/substitution/primitive-operation charges with deterministic weighted `BillableTokenEvent`s or documented non-consensus admission limits (Section 5.9.1). System deploys run under an explicit unmetered/no-op budget; they must not keep `CostManager` as a second runtime metering path. Depends on steps 15 and 16 (the old code is no longer on any user deploy code path).

18. **Add structural equivalence tests** verifying that deploys differing only by commutativity or associativity of parallel composition produce identical `TokenBudget.total_cost()` (Section 5.7). Depends on steps 15 and 16.

19. **Deploy Split/Join mediator processes** to the test network. Depends on step 11.

20. **Run integration tests** on a private test shard, including the complete-replacement acceptance criteria in Section 5.9.4. Depends on steps 12–19.

21. **Benchmark throughput:** measure deploys/second with the new `FuturesUnordered` eval loop on the standard Rholang benchmark suite. Depends on step 20.

22. **Coordinate network-wide activation** via a block-height trigger (hard fork). At block height `H_activation`, the internalized cost model becomes the sole cost model across all validators. Depends on step 20 (all integration tests pass) and step 21 (throughput regression is acceptable).

23. **Monitor the network** post-activation: validator cost agreement should hold by `ca_cost_deterministic` (no recurrence of the order-dependent class even theoretically); track throughput metrics (deploys/second, block fill rate) and latency metrics (deploy-to-finalization time) and compare against the externalized-model baseline to quantify the per-COMM bookkeeping overhead reclaimed. Depends on step 22.

24. **Remove diagnostic logging and debug assertions** that were added for migration testing but are no longer needed. Update documentation to reflect the new architecture as the sole cost model. Depends on step 23 (sustained correct operation confirmed).

---

## 7. Formal Correctness (Pointer to Verification Doc)

The formal correctness argument for this migration lives in the
verification companion, [*Formal Verification of Cost-Accounted Rho
Calculus*](cost-accounted-rho-verification.md):

- **Rocq proofs** — verification doc §6 (headline theorems), §7 (proof
  architecture), §8 (proof techniques), §9 (end-to-end mathematical
  proofs). 23 modules, 18,550 lines, 624 `Qed.`/`Defined.` proof terms,
  zero admissions and zero axioms. The consensus-critical theorems are
  unconditional; replication support is scoped to the one-step
  reflective unfold and axiom-free forward weak-barb propagation theorem.
- **TLA+ model checking** — verification doc §10 (TLA+ Correctness
  Model), covering eight TLA+ specifications: the four core
  protocol/scheduling models (`CostAccountedRho.tla`,
  `CompoundProtocol.tla`, `FullProtocol.tla`, `EvalScheduling.tla`) plus
  `RuntimeBudgetReplay.tla`, `CostAccountingThreats.tla`,
  `CostAccountingSearchFrontier.tla`, and
  `MergeableChannelAccounting.tla`. TLC verified every invariant
  across every reachable state (up to 12,960 distinct states for the
  most comprehensive core protocol configuration, 5,408 distinct states
  for the slash-authorization threat model, and 34,167 distinct states
  for the source-graph search-frontier model). Apalache also accepts the
  typed threat/search-frontier models with `NoError` at the bounded
  symbolic check depth.
- **Assumptions and trust base** — verification doc §12.

Operational coverage for the migration is cataloged in
[*Cost-Accounted Rho Use-Case Coverage*](cost-accounting-use-cases.md).
That catalog maps each consensus-critical use case to its Rocq/TLA+
anchor and to the `f1r3node-rust` test target that exercises the
production boundary.

The key theorems this migration relies on are:

| Property the migration depends on | Verification-doc theorem             | Guaranteed regardless of |
|-----------------------------------|--------------------------------------|--------------------------|
| Token conservation (monotonicity) | `token_monotone_step`, `token_monotone_reachable` | Scheduling, signature shape |
| Cost determinism across validators| `ca_cost_deterministic`              | Schedule, deploy composition |
| Step determinism per deploy       | `ca_step_deterministic`              | Schedule (single-token case) |
| Fuel-gate safety (capability)     | `fuel_gate_stuck_isolated`           | Any user process under the gate |
| Gate-level backward reflection    | `backward_reflection_phased_gate`    | Signature shape; Split routing |
| Whole-system backward reflection for the implementation target | `well_reflected_backward_reflection` | Any rho step from a recursively metered image |
| Source billing witness            | `billed_step`, `ca_step_billed`      | Rule choice; token layout |
| Strong normalization              | `ca_strongly_normalizing`            | Input system size            |
| Full confluence                   | `ca_confluent` (via `newman`)        | Divergent execution paths    |
| Fuel-event multiset determinism   | `fuel_events_consumed_perm`          | Schedule, redex interleaving |
| Channel separation                | `N_tr_is_Quote`, `fuel_gate_no_app_channel_overlap` | User-code channel names |

The migration changes the *implementation* of cost accounting from an
external `ChargingRSpace` wrapper to recursive metering with
continuation-keyed gates; the verification doc certifies that the new
implementation target (`well_reflected` cost-accounted systems modulo
the recursive metering relation) satisfies the properties above
unconditionally.

Backward reflection now has two layers. The legacy compositional
translation has a phase-based gate theorem:
`backward_reflection_phased_gate` proves that any pure-rho step out of a
well-formed translated gate reaches the unique spent phase and accounts
for exactly one billable source-token event, even when the first target
step is compound-signature Split routing. The implementation target uses
the stronger recursive invariant `well_reflected`; the theorem
`well_reflected_backward_reflection` proves that every pure-rho step
from a recursively metered image corresponds to an actual source
`ca_step` and lands in another recursively metered image. The current
implementation design must therefore instantiate the recursive metering
kernel, not count raw target-side COMM events from an arbitrary `S_tr`
image.


---

## 8. Consensus Implications

The architectural implications of internalizing cost accounting are discussed in [4, Section 6], which identifies four benefits: cost control as a Rholang program (6.1), composable metering (6.2), formal verification (6.3), and an implementation path (6.4). This section addresses the consensus-specific consequences.

### 8.1 What Changes

**Cost computation.** The cost of a deploy changes from "sum of `storage_cost_*` charges and refunds minus event storage costs" to "sum of deterministic billable source-token event weights." The runtime observes those events as fired recursive metered gates and weighted primitive/source events, and must exclude pending registrations, continuation scheduling, and Split/Join mediator COMMs. The numerical value of the cost will differ between the two models for most deploys. This is a consensus-breaking change that requires coordinated network activation (Section 6, step 22).

**Cost determinism guarantee.** F1R3Node's `ChargingRSpace` already makes cost order-independent empirically: `cost_should_be_deterministic` re-runs each contract 20 times, `cost_should_be_repeatable_when_generated` runs 10,000 randomly-generated contracts, and `peek_with_parallel_produce_should_have_deterministic_replay_cost` directly guards the peek-with-parallel-produce class (`rholang/tests/accounting/cost_accounting_spec.rs:323,344,474`). Under the internalized model, cost determinism is upgraded to a machine-checked theorem (`ca_cost_deterministic`), proven via strong normalization plus local confluence composed through Newman's lemma. This is a strictly stronger guarantee than empirical test coverage: it holds for every reachable state, not just the sampled ones.

**Event hashing.** The events logged by the tuple space for replay and consensus purposes change. In the externalized model, events include produce and consume operations with their associated storage costs. In the internalized model, the cost-relevant events are fired recursive metered gates. Because the implementation intentionally maximizes parallelism, validators must not hash these events in completion order. The event digest must be computed from a deterministic canonicalization of the source-token events, such as sorting by deploy id, source-path/redex id, and local step index, or by a collision-resistant multiset commitment over the same descriptors. The Rocq theorem `fuel_events_consumed_perm` supplies exactly the multiset-determinism property this design needs.

*Discriminating billable gates from mediator firings.* The Rocq cost notion is "one `ca_step` per fired recursive metered gate." The source `ca_step` relation is defined on the cost-accounted system (`SSigned | SToken | SPar`), which contains no Split or Join. Split and Join are `proc`-level mediators in the legacy translation target; they shuffle tokens between signature channels but do *not* correspond to `ca_step` reductions. The Rust evaluator therefore must count only the continuation-keyed gates installed by the recursive metering kernel, where the key is tied to a selected source successor. Legacy `P_tr` fuel-gate firings remain useful for local proof correspondence, but they are not sufficient as the whole-system billing discriminator. Mediator receivers (`Split`'s continuation `N⟦s₁⟧!(0) ∣ N⟦s₂⟧!(*t)`, `Join`'s continuation `N⟦s₁ & s₂⟧!(*t₁ ∣ *t₂)`) are syntactically distinct and must be excluded from the cost count and from the fuel-event digest.

For a single linear deploy, the token-chain structure still induces the expected gate sequence. For maximally parallel deploys, however, branch completion order is intentionally not fixed. The deploy's fuel-event digest must therefore be derived from canonical source-event descriptors after evaluation, not from wall-clock completion order. A safe shape is:

```
descriptor = blake2b256(deploy_id || source_path || redex_id || token_kind)
event      = descriptor || local_index || kind || weight || success_or_oop_tag
digest     = blake2b256(sort(events))
```

where `source_path` is a deterministic path through the normalized `SPar`
tree, `redex_id` identifies the selected source redex, and
`success_or_oop_tag` distinguishes successful reservations from the single
out-of-phlogistons boundary event. The stored replay payload includes both
the digest and the event count, so truncation or event insertion cannot
silently collide with an empty or shorter trace. This preserves parallel
scheduling freedom while keeping validator-visible replay data deterministic.

The intra-deploy ordering is *paper-original*: the token-chain encoding `T⟦σ:T'⟧ = N⟦σ⟧!(T⟦T'⟧)` ([4, §4.2]) places at most one token message on any signature channel at a time, and each fuel-gate firing dequotes the next token via `*t` to release the subsequent gate. Validators executing the same translated process therefore observe the same fuel-event sequence not because they *agree* on an order, but because the process structure admits exactly one. The Rocq theorems verify this property: `ca_step_deterministic` (`StepDeterminism.v:156`) shows that any single-token system has a unique successor at each step; `single_token_path_unique` (`StepDeterminism.v:249`) lifts this to whole reduction paths; and `fuel_events_consumed_perm` (`FuelEventDecomposition.v:198`) proves the broader multiset-equality property from which list-equality drops out in the single-token case. None of these theorems introduce the ordering — they verify the algorithm the paper specifies. The cross-deploy case, by contrast, is *not* a paper claim: block-level ordering of per-deploy digests is a F1R3Node deployment choice (canonical deploy-index order, enforced by validator code), and what the Rocq proof contributes there is `ca_cost_deterministic` for the parallel composition of all in-block deploys, which guarantees the *aggregate* token count is order-independent. The block-level ordered hash chain therefore relies on (a) the paper's intra-deploy ordering algorithm (verified in Rocq) plus (b) protocol-level inter-deploy ordering by deploy index; both are required, and the paper-vs-protocol distinction should be kept explicit in any audit.

Across deploys in a block, the deploy-level digests are combined in canonical block order (deploy index order) using the same incremental hash chain, yielding a block-level fuel-event digest. Application-level events continue to use the existing ordered event log (they are replayed in sequence via rig-and-reset and do not require any special hashing treatment).

**Phlogiston pricing.** The phlogiston cost of a deploy under the new model is the sum of deterministic source-token event weights reserved by `TokenBudget`. Direct rho steps use the Rule 1-5 source-token counts; primitive operations and other consensus-billable work use explicit deterministic weights (Section 5.9.1). This is a different cost metric than the protobuf-encoded size of stored data. Pricing and rate-limiting policies must be recalibrated against the token-weight schedule before activation.

**Cost validation.** Under the externalized model, validators do not currently verify deploy costs — the only cost-related check is `phlo_price >= min_phlo_price` (`validate.rs`). Under the internalized model, cost is deterministic (by `ca_cost_deterministic`), so validators can and should verify cost correctness. During block validation, each validator re-executes the recursive metering protocol and compares the resulting `TokenBudget.total_cost()` with the cost field in the block's `ProcessedDeploy`. A mismatch indicates a faulty or malicious block proposer and is grounds for rejecting the block.

**Replay transparency.** Recursive metered gate firings are standard COMM events in the translated pure rho calculus — they produce and consume data on continuation-keyed internal channels via the same `produce`/`consume` operations used by application-level communication. The rig-and-reset replay mechanism (`replay_rspace.rs`) partitions the event log into IO events (produces, consumes) and COMM events, then re-indexes them for deterministic replay. Metered-gate COMMs appear in this log as ordinary internal COMM events, while their cost digest is derived from canonical source-event descriptors rather than completion order. The matching logic itself remains the same as for all other COMMs.

**Token sweep determinism.** The post-evaluation token sweep (Section 5.8.2) removes internal fuel artifacts from the deploy's signature channel(s) before the hard checkpoint. Because signature channels are scoped per-deploy, and because the sweep removes **all** metering data and unfired gate continuations on those channels, the sweep is deterministic: every validator performs the same removal on the same channels, yielding the same post-sweep tuple-space state. The sweep operation itself is recorded in the event log as a sequence of deterministic removals, which are replayed during rig-and-reset. Validators that replay a block containing a deploy with residual metering artifacts will replay both the fuel-gate COMMs and the sweep removals, arriving at the same final state.

### 8.2 What Does Not Change

**Block structure.** Blocks still contain deploys, and deploys still have a cost field. The cost field now stores the token count rather than the accumulated phlogiston charges, but the block format is unchanged.

**The COMM rule.** The fundamental computational step of the rho calculus is unchanged. A receiver and a sender on the same channel fire, producing a substituted body. The fuel-gate mechanism adds a layer of COMM events (on signature channels) above the application-level COMM events, but the rule itself is the same.

**Validator model.** Validators still propose blocks, validate deploys, and check costs. The validation logic changes (validators compare token counts instead of accumulated charges), but the validator protocol is unchanged.

**Replay.** The tuple space replay mechanism (rig-and-reset) continues to work. Events are replayed in the same order, and the cost is deterministic regardless of replay order (by token conservation).

**Rholang language.** No changes to the Rholang language syntax or semantics. The recursive metering mechanism is inserted by the compiler/runtime's internal pass (between normalization and reduction), which is completely transparent to the Rholang programmer. Existing Rholang programs run unchanged with zero source modifications. A developer writing `new ch in { ch!(42) | for(@x <- ch){ stdout!(x) } }` today writes exactly the same code after the migration. The `Sig`, `Token`, and `SignedProcess` types are internal compiler IR — they never appear in Rholang source code, error messages, or developer-facing APIs.

**System deploy handling.** System deploys (genesis, slash, close-block, heartbeat, and fee-settlement deploys) are exempt from the cost-accounting translation and run without fuel gates under an explicit unmetered/no-op budget. They are not a reason to retain `CostManager` as a second runtime metering path. System deploys are identified by `is_system_deploy_id(deploy_id)` (`system_deploy.rs`) and bypass the Signature Annotator entirely. Slash system deploys may consume invalid-block evidence produced by cost validation or replay mismatch only when the evidence epoch and target activation epoch are current; the target epoch is part of the authenticated replay payload. The slash effect is confined to PoS slashing state and preserves user deploy fuel, token cost, and fee settlement inputs.

**Typed mergeable-channel handling.** Mergeable channels now carry an explicit `MergeType`. `IntegerAdd` preserves the existing additive diff/merge path for numeric channels. `BitmaskOr` is used for registry-style bitmaps: multi-value observation OR-folds all numeric values, diffs record newly-set bits as `end & !previous`, and merge replays with OR so no set bit is lost. Tagged non-numeric values are not coerced into the numeric merge path; they fall through to the ordinary conflict-rejection path. The mergeable-channel cache persists the merge type alongside each diff, and the formal model proves this metadata update preserves user fuel and fee-settlement evidence.

### 8.3 Backward Compatibility

Existing deploys submitted before the activation block height `H_activation` are validated using the externalized cost model. Deploys submitted after `H_activation` are validated using the internalized model.

For backward compatibility of existing deploy patterns, the Signature Annotator wraps legacy deploys in an internal unit-signature `SignedProcess`:

```
Legacy deploy P with fuel limit n
  ⟹  SSigned(P, SUnit) ∥ SToken(repeat n (TGate SUnit) TUnit)
```

The legacy compositional translation has the familiar one-gate local shape:

```
S_tr(SSigned(P, SUnit) ∥ SToken(TGate(SUnit, TUnit)))
  = P_tr(P, SUnit) | T_tr(TGate(SUnit, TUnit))
  = for(t ← @0){ lift(P,1,0) | *t } | @0!(0)
```

That local gate fact is still used for traceability and fuel-gate safety,
but it is not the whole migration pricing rule. The recursive metering
kernel re-enters the metered evaluator after each source successor, so a
legacy deploy consumes one token for each selected source `ca_step` until
it terminates or exhausts its fuel limit. The process syntax and behavior
remain backwards-compatible for Rholang developers; the cost field is the
new token count, activated at the coordinated network boundary.

The Rocq formalization confirms this in the `unit_translation_one_step_to_body` (`Bisimulation.v:279`) and `unit_post_gate_canonical` (`Bisimulation.v:244`) theorems, together with `unit_translation_strong_bisimilar` (`Bisimulation.v:761`): the translation of a unit-signed process takes exactly one fuel-gate step, reaches a canonical post-gate form, and is strongly bisimilar to the body `P` (modulo structural equivalence with a released nil payload).

The whole-system pricing rule is instead justified by
`well_reflected_backward_reflection`: every fired recursive metered gate
reflects to exactly one source step, and the continuation remains inside
the recursively metered invariant.

### 8.4 Composable Metering Patterns

Paper [4, §6.2] enumerates four composable-metering strategies enabled by the internalized cost model. The F1R3Node implementation supports them as follows:

| Paper pattern         | Mechanism                                                                                                              | Implementation status                                                                              |
|-----------------------|------------------------------------------------------------------------------------------------------------------------|----------------------------------------------------------------------------------------------------|
| **Flat-rate**         | One recursive metered gate firing per selected source step, regardless of payload size or computation complexity.       | Default for the migration. Directly supported by `recursive_metered_gate` and `well_reflected_backward_reflection`. |
| **Proportional**      | Token-chain length encodes the fuel balance; each gate firing dequotes the next token, debiting one unit per firing.   | Directly supported by the recursive token grammar `T ::= () \| s:T` and `T_tr` translation.        |
| **Delegated**         | One signature funds another's computation: a deployer deposits tokens on the recipient's signature channel.            | Achievable as a user-space pattern (signature channels are open names once published); not specifically formalized. |
| **Market-based**      | Token prices float based on demand for particular signature channels, implementing a fee market entirely within Rholang. | Achievable as a user-space pattern atop signature channels; not specifically formalized.           |

The migration's v1 scope covers flat-rate and proportional metering — both are directly enabled by the recursive metering pass and require no additional Rholang language features. Delegated and market-based metering become available without further runtime support, since the signature-channel and token APIs they require are already exposed by the internal cost-accounted IR; they are user-space contract patterns rather than implementation deliverables.

---

**Appendix A: Rocq Module Inventory and Rocq ↔ TLA+ Correspondence**

The per-module inventory (file-by-file list of key results and line
counts) is documented in the verification companion,
[verification doc §11.1](cost-accounted-rho-verification.md#111-file-listing).
The Rocq ↔ TLA+ property correspondence table (which Rocq theorem
corresponds to which TLA+ invariant) is in
[verification doc §10.5](cost-accounted-rho-verification.md#105-rocq--tla-correspondence).
The per-theorem dependency table on the `hash_process` encoding
parameter and three section hypotheses is in
[verification doc §12.1](cost-accounted-rho-verification.md#121-explicit-assumptions-section-hypotheses).

**Appendix B: The Five Cost-Accounted Reduction Rules**

For reference, the five rules of the cost-accounted calculus (from `CostAccountedReduction.v`):

**Rule 1** — Atomic signature, joined redex, single token:
```
(for(y ← x) P ∣ x!(Q))^s ∣ s:T   ⤳   (P{@Q/y})^s ∣ T
```

**Rule 2** — Compound signature, joined redex, split tokens:
```
(for(y ← x) P ∣ x!(Q))^{s₁ & s₂} ∣ s₁:T₁ ∣ s₂:T₂
    ⤳   (P{@Q/y})^{s₁ & s₂} ∣ T₁ ∣ T₂
```

**Rule 3** — Compound signature, joined redex, combined token:
```
(for(y ← x) P ∣ x!(Q))^{s₁ & s₂} ∣ (s₁ & s₂):T
    ⤳   (P{@Q/y})^{s₁ & s₂} ∣ T
```

**Rule 4** — Compound signature, split processes, combined token:
```
(for(y ← x) P)^{s₁} ∣ (x!(Q))^{s₂} ∣ (s₁ & s₂):T
    ⤳   (P{@Q/y})^{s₁ & s₂} ∣ T
```

**Rule 5** — Compound signature, split processes, split tokens:
```
(for(y ← x) P)^{s₁} ∣ (x!(Q))^{s₂} ∣ s₁:T₁ ∣ s₂:T₂
    ⤳   (P{@Q/y})^{s₁ & s₂} ∣ T₁ ∣ T₂
```

In all rules, `P{@Q/y}` denotes the substitution of the quoted name `@Q` for the bound variable `y` (de Bruijn index 0) in process `P`. The signature on the left-hand side authorizes the computation; the token(s) provide the fuel. After the step, the token(s) advance by one gate (stripping the outermost `TGate` constructor), reducing the system's total fuel by 1 (Rules 1, 3, 4) or 2 (Rules 2, 5).

**Appendix C: File Locations**

Implementation paths, formal-verification artifacts, and regression tests are co-located in this repository.

| Component       | Path                                                                  | Repository |
|-----------------|-----------------------------------------------------------------------|------------|
| RuntimeBudget and cost trace digest | `rholang/src/rust/interpreter/accounting/mod.rs` | this repository |
| MeteredMachine  | `rholang/src/rust/interpreter/metering.rs`                             | this repository |
| Cost helpers retained for compatibility/diagnostics | `rholang/src/rust/interpreter/accounting/{costs.rs,has_cost.rs,cost_accounting.rs}` | this repository |
| Interpreter entrypoint | `rholang/src/rust/interpreter/interpreter.rs`                  | this repository |
| Substitution source-event path | `rholang/src/rust/interpreter/substitute.rs`             | this repository |
| Parallel eval loop | `rholang/src/rust/interpreter/reduce.rs`                           | this repository |
| Runtime construction | `rholang/src/rust/interpreter/rho_runtime.rs`                    | this repository |
| SoftCheckpoint  | `rspace++/src/rspace/checkpoint.rs`                                   | this repository |
| Casper runtime  | `casper/src/rust/rholang/runtime.rs`, `casper/src/rust/rholang/replay_runtime.rs` | this repository |
| Replay cache/hash | `casper/src/rust/util/rholang/runtime_manager.rs`                   | this repository |
| ProcessedDeploy cost/refund | `models/src/rust/casper/protocol/casper_message.rs`     | this repository |
| Deploy validation | `casper/src/rust/validate.rs`                                       | this repository |
| Cost-acc deploys| `casper/src/rust/util/rholang/costacc/`                               | this repository |
| Retired externalized baseline | `CostManager` / `ChargingRSpace` design roles; no live `charging_rspace.rs` source in this branch | historical design context |
| Deterministic-cost tests | `rholang/tests/accounting/cost_accounting_spec.rs`           | this repository |
| Cost-accounting frontier tests | `rholang/tests/accounting/cost_accounting_frontier.rs`  | this repository |
| Rocq proofs     | `formal/rocq/cost_accounted_rho/theories/*.v`                         | this repository |
| TLA+ models     | `formal/tlaplus/cost_accounted_rho/*.tla`                             | this repository |
| TLA+ MC configs | `formal/tlaplus/cost_accounted_rho/MC*.tla`                           | this repository |

---

## 9. References

[1] L. G. Meredith and M. Radestock, "A reflective higher-order calculus," *Electronic Notes in Theoretical Computer Science*, vol. 141, no. 5, pp. 49-67, 2005. DOI: [10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016)

[2] R. Milner, *Communicating and Mobile Systems: the pi-Calculus*. Cambridge University Press, 1999. DOI: [10.1017/CBO9780511811753](https://doi.org/10.1017/CBO9780511811753)

[3] L. G. Meredith *et al.*, "Rholang Specification," F1R3FLY.io, 2017-2026. The F1R3Node implementation currently lives in this repository (the standalone Rust port that replaces the prior Scala/Rust hybrid): [https://github.com/F1R3FLY-io/f1r3node-rust](https://github.com/F1R3FLY-io/f1r3node-rust). Historical upstream: [https://github.com/F1R3FLY-io/f1r3node](https://github.com/F1R3FLY-io/f1r3node).

[4] L. G. Meredith, "Translating Cost-Accounted Rho Calculus Back to the Pure Rho Calculus," F1R3FLY.io, April 2026. Mechanized in Rocq at `formal/rocq/cost_accounted_rho/` (23 modules, 18,550 lines, 624 `Qed`/`Defined` proof terms, zero admissions, zero axioms). TLA+ finite-state model at `formal/tlaplus/cost_accounted_rho/` (verified by TLC with zero errors, with typed threat/search-frontier models also accepted by Apalache). See the verification companion, [*Formal Verification of Cost-Accounted Rho Calculus*](cost-accounted-rho-verification.md).

[5] L. Lamport, *Specifying Systems: The TLA+ Language and Tools for Hardware and Software Engineers*. Addison-Wesley, 2002. ISBN: 0-321-14306-X.

[6] D. Sangiorgi and D. Walker, *The pi-Calculus: A Theory of Mobile Processes*. Cambridge University Press, 2001. DOI: [10.1017/CBO9780511755149](https://doi.org/10.1017/CBO9780511755149)

[7] G. Winskel, *The Formal Semantics of Programming Languages: An Introduction*. MIT Press, 1993. ISBN: 0-262-23169-7.

[8] B. C. Pierce, *Types and Programming Languages*. MIT Press, 2002. ISBN: 0-262-16209-1.
