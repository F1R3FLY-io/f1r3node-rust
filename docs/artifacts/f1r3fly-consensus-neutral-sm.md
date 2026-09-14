# F1r3fly: Parallel State Machines and Consensus-Neutral Execution

- **Status:** internals architecture note
- **Audience:** F1r3fly contributors
- **Home in-tree:** `docs/artifacts/f1r3fly-consensus-neutral-sm.md`
- **Sources in this repository:** the `casper` crate, [`docs/casper/CONSENSUS_PROTOCOL.md`](../casper/CONSENSUS_PROTOCOL.md), and [`docs/Glossary.md`](../Glossary.md)
- **External sources:** the Pyrofex Casanova specification ([arXiv:1812.02232](https://arxiv.org/abs/1812.02232)), RGB client-side validation in the private F1r3fly-RGB tree with Lightning and Bitcoin UTXO seals, and Cordial Miners ([arXiv:2205.09174](https://arxiv.org/abs/2205.09174), cited only)
- **Maturity labels:** Smart Assets workspace tiers (`coming_soon`, `prototype`, `alpha`, `demo`, Production) per [feature-maturity-and-mock-data.md](https://gitlab.com/smart-assets.io/gitlab-profile/-/blob/master/docs/standards/feature-maturity-and-mock-data.md)

---

## 1. Claim

F1r3fly does not implement classical Byzantine state-machine replication. In that model, a consensus log is the input tape of one replicated machine, and every honest replica executes the same total order.

The load-bearing claim is stronger than "swap the Casper engine behind a trait":

> **Rholang reduction and RSpace produce and consume do not depend on a total order of deploys, except where names collide.** Non-conflicting deploys commute. Different ordering media may propose, merge, seal, or client-validate them. No global replica log of contract commands is necessary.

Consensus is an **ordering and uniqueness medium**. Execution is a **parallel process calculus** over a tuple space. In the RGB path, execution is a parallel client-side contract machine bound to single-use seals. The two compose at a thin coupling surface: a block post-state hash, a seal close, or a blocklace τ-order. The VM is not embedded in the consensus protocol.

The node already documents this cut for CBC Casper:

| Consensus-specific | Consensus-agnostic (reusable) |
| --- | --- |
| Fork choice (LMD GHOST) | DAG storage |
| Safety oracle (clique oracle) | Block persistence |
| Finalization (FT threshold) | Deploy pool |
| Synchrony constraint | Contract execution (`RhoRuntime` and `ReplayRSpace`) |
| Equivocation detection | P2P transport |
| Pre-proposal constraint checks | Engine trait (`Arc<dyn MultiParentCasper>`) |

RGB, Casanova, and Cordial Miners are additional *media* that can occupy the left column. They do not replace RSpace.

---

## 2. What "consensus-neutral" means here

**Means**

- The ρ-calculus reduction and RSpace matching define the execution semantics. The round structure of the consensus algorithm does not.
- Commuting deploys need no agreed total order. The merge and conflict machinery runs only on colliding deploy signatures or colliding names.
- An ordering medium may be a multi-parent PoS DAG (Casper) or a Bitcoin and Lightning seal graph (RGB). It may also be a leaderless optimistic blockDAG (Casanova) or a blocklace (Cordial Miners).
- The medium that uniquely closes the relevant conflict set supplies the finality of *execution effects*. That close is a clique oracle, a seal spend, an FTM lock, or a τ-order. The VM does not invent uniqueness.

**Does not mean**

- "No Layer 1." Every path still needs a publication or uniqueness substrate.
- "Casper is optional in today's node." The shipping engine is CBC Casper in `casper`. The other media are specified, partial, or cited only.
- "All four protocols implement the same SMR problem." They do not. They solve different uniqueness and order problems for the same execution layer.

**Engine boundary that already exists**

`engine.rs` holds `Arc<dyn MultiParentCasper>` in the `Running` state. That trait object is the in-tree handle for the estimator, oracle, and finalizer that sit in front of RSpace. It is not the full consensus-neutral theorem. It is the implementation hinge for Casper-shaped DAG engines. RGB is not a `MultiParentCasper` implementation. RGB is a second machine plus a seal medium.

---

## 3. Two machines, four media

### 3.1 Machines (execution)

Two execution machines are in scope. Ordering protocols are not additional machines.

| Machine | State | Transition | Who executes | Who must see the history |
| --- | --- | --- | --- | --- |
| **RSpace and Rholang** | Tuple space: data and waiting processes on names | Produce, consume, and peek, with concurrent `Par` | Every validator that replays a block on the Casper path, or the local runtime otherwise | Validators in the shard that finalize the post-state |
| **RGB contract SM** | Per-contract DAG from genesis to owned assignments | Schema-valid state transition closed over a single-use seal | Counterparties through client-side validation, and the F1r3fly-RGB stash | Holders of the relevant consignment, not the Bitcoin network |

A third *mode* of the rho runtime exists: simulation or inference versus an observable RSpace commit. That is an interpreter distinction. It is not a third consensus machine and not a fourth ordering medium.

Non-conflicting RSpace deploys commute. If two deploys do not share consume or produce names in a conflicting way, their parallel composition is a valid `Par`. Casper, Casanova, and a seal need not serialize them. Conflicting deploys are a merge problem in Casper (`ConflictSetMerger`) or a line-item veto in Casanova. In RGB and in blocklace equivocation, they are a second close of the same seal.

### 3.2 Media (order and uniqueness)

| Medium | Structure | What it uniquely decides | Conflict isolation | Coupling to execution |
| --- | --- | --- | --- | --- |
| **CBC Casper** (`casper`) | Multi-parent justification DAG | The post-state hash of a merged parent set, and the LFB through the clique oracle | Deploy-signature conflicts at merge, and slashing for equivocation | Replay RSpace against the claimed post-state |
| **RGB seals** | Bitcoin TxO2 and Lightning channel UTXOs as single-use seals | That a given seal closed over at most one committed message | Double-spend of the UTXO or channel output | A commitment (opret, tapret, or LN state) to an RGB transition. Rho names may be sealed the same way |
| **Casanova** | Leaderless PoS blockDAG | An FTM lock on a *conflicting* transaction set, while non-conflicting blocks confirm in parallel | Line-item veto: only the conflict enters extra voting | Specified to sit in front of the same execution layer, not implemented in `f1r3node-rust` |
| **Cordial Miners** | Blocklace (partial order first, τ total order later) | Dissemination, equivocation exclusion, and τ-order without Reliable Broadcast | Equivocation lives in the lace, with cordial dissemination | Cited only. The same "partial order first" story as the RGB seal DAG |

---

## 4. Casper (shipping DAG engine)

**Maturity:** `demo`

The `casper` crate in this repository is the implemented consensus engine. It covers block creation, validation, DAG management, the clique safety oracle, slashing, finalized-floor merge scope, and soak-gated shards. It is F1r3fly CBC Casper. It is not the RChain node and not Ethereum Casper FFG.

Properties that matter for this note:

- Each block may cite multiple parents. Forks merge. They are not discarded.
- Fork choice is LMD GHOST over latest messages and stake.
- Finality is the clique oracle: FT from the maximum weighted agreement clique, as an exact integer threshold against the genesis-locked θ.
- Execution runs *inside* the proposal and validation pipelines, but the pipelines do not *define* it. The play runtime runs on propose, `ReplayRSpace` runs on validate, and the post-state hash is the agreement object.
- Multi-parent merge (`dag_merger` and `conflict_set_merger`) is where commutativity becomes operational. The merge sees visible blocks above the finalized floor and detects conflict on deploy signature. It adjudicates with loss awareness and retries rejected signatures through the rejected-deploy buffer. Non-conflicting deploys from co-parents both land.
- Merge scope is a function of the block's frozen justifications (the floor), not of a node-local LFB. Merge therefore stays deterministic across validators with different local finality views.

Casper is therefore already a **partial-order engine with a total-order-on-conflicts overlay**. That is why Casanova and Cordial are adapters in spirit, not a rewrite of RSpace.

Casper is **not Production** under the workspace standard. The soak shards and the standing test net are sandbox and testnet backends (`demo`), not an unmarked production backend.

---

## 5. RGB (parallel SM plus seal medium)

**Maturity:** `alpha`

F1r3fly-RGB is both:

1. A **client-side contract state machine** with a schema, a transition DAG, consignments, and a stash.
2. A **seal medium** for those transitions, which uses **Bitcoin UTXOs and Lightning outputs**.

Bitcoin and Lightning nodes do not store or reduce RGB state. They uniquely spend an output. The spend is the seal close. The message closed over is the RGB transition. Where F1r3fly binds them, the message also commits that a rho name or assignment is now held by that output.

This matches the earlier architectural cut:

- Layer 1 and LN supply uniqueness and a timestamp for the seal closure.
- The RGB SM supplies per-contract, client-validated transitions.
- The two compose at the seal or commitment. AluVM and schema validation do not enter `casper`.

F1r3fly-specific consequences:

- RGB contracts are shards by construction. Unrelated contracts do not share a log.
- A rho deploy that does not touch a sealed name does not wait on a Bitcoin confirmation.
- A transfer that *does* move a sealed assignment waits on the witness transaction or the LN state update. That update closes the previous seal and defines the next.
- The on-chain order of witness transactions is not the RGB DAG order. This is the same fact as in RGB generally. F1r3fly must not use the Casper block number as a substitute RGB clock.

The RGB repository may be private. Treat in-tree public code as absent until that tree is published next to `f1r3node-rust`.

The tier is `alpha`, not `prototype`. Partial real services exist, in the form of actual Bitcoin and Lightning seal closes. Client-side validation and the stash remain the source of contract state.

---

## 6. Casanova (specified optimistic DAG)

**Maturity:** `coming_soon`

Casanova (Butt, Sorensen, Stay, Pyrofex, [arXiv:1812.02232](https://arxiv.org/abs/1812.02232)) is a **leaderless optimistic** consensus protocol for a permissioned PoS setting. It is specified. It is **not** implemented in `f1r3node-rust`. Work toward an adapter is in progress. Do not cite an in-tree crate.

Design that matters for consensus neutrality:

- Blocks form a DAG, not a single chain. Members produce blocks in parallel.
- The happy path records transactions without a per-block leader race.
- The protocol singles out conflicts, such as a double spend or mutually exclusive transactions. Consensus voting concentrates on the conflict. Non-conflicting transactions in the same block do not pass through a full block-invalidation path. This is the "line-item veto".
- Safety holds under asynchrony. Liveness holds under partial synchrony. FTM (fault-tolerant majority) locks decide.

Casanova is the protocol that most directly states what the Casper merge already does. Do not discard a block because one deploy collided. Isolate the collision. A future adapter must reuse RSpace and the conflict records, not invent a second tuple space.

Until a `MultiParentCasper` implementation or a sibling engine crate exists, Casanova stays documentation and specification only. Interactive UI that suggests otherwise is out of tier, since `coming_soon` is non-interactive.

---

## 7. Cordial Miners (citation, blocklace)

**Maturity:** `coming_soon`

Cordial Miners (Keidar, Naor, Poupko, Shapiro, [arXiv:2205.09174](https://arxiv.org/abs/2205.09174)) is a family of Byzantine atomic-broadcast protocols. They drop Reliable Broadcast and use a **blocklace**, a partially ordered generalization of a blockchain. The lace does dissemination, equivocation exclusion, and ordering. τ is the later total order. Eventual-synchrony and asynchronous instances exist.

**This note cites the paper only.** A parallel repository is expected later. Research adapters such as `cordial-f1r3node` are not F1r3fly canonical until they live under `F1R3FLY-io` and implement the engine boundary. There is no in-tree wiring.

Why the citation belongs here:

- The blocklace is the DAG analogue of the RGB seal DAG: **partial order first, total order only where needed**.
- Equivocation is a first-class lace object, not a hidden Reliable Broadcast assumption. That matches the explicit equivocation detector and slashing pipeline in Casper.
- The three Cordial consensus jobs, disseminate, exclude equivocation, and order, are exactly the left column of the Casper abstraction table. Execution stays off that table.

Do not describe Cordial as "F1r3fly consensus" in user-facing text until the parallel repository and an engine adapter exist.

---

## 8. How the pieces compose

```text
                    ┌─────────────────────────────────────┐
                    │     Rholang + RSpace (machine A)    │
                    │  concurrent Par; commute if no name │
                    │  collision; replay → post-state hash│
                    └──────────────┬──────────────────────┘
                                   │ effects / names
          ┌────────────────────────┼────────────────────────┐
          │                        │                        │
          ▼                        ▼                        ▼
   Casper DAG                 RGB SM (machine B)      (future)
   LMD GHOST                  schema + consignments   Casanova DAG
   clique oracle              seal close on           line-item veto
   merge + slash              BTC UTXO and LN         FTM lock
          │                        │                        │
          └────────────┬───────────┴────────────┬───────────┘
                       │                        │
                       ▼                        ▼
              uniqueness / finality      Cordial blocklace
              of the conflict set        (citation; τ-order)
```

**Shared surfaces (keep them small)**

- The agreement *object* for Casper: the post-state hash, the justifications, and the bonds.
- The agreement *object* for RGB: a seal close over a transition commitment.
- The conflict *key* for the Casper merge: the deploy signature.
- The conflict *key* for RGB: the same UTXO or LN output spent twice.
- The conflict *key* for Casanova: an explicit conflicting transaction pair.
- The conflict *key* for Cordial: equivocating blocks in the lace.

**Do not share**

- The Casper block number as RGB time.
- The Bitcoin transaction order as the RSpace reduce order.
- The clique-oracle FT as a substitute for a seal close.
- The τ-order as a requirement on commuting `Par` terms.

---

## 9. Verification by machine and medium

The formal tree and the CbC tags follow the same cut. Each check covers one machine, one medium, or one shared substrate. Each future repository keeps its own gate registry, evidence ledger, and claim documents. Cross-citations become pinned references, in the same form as the system-integration pin.

| Check | Formal areas | CbC tags | Shared by |
| --- | --- | --- | --- |
| Machine A, execution | merge algebra, rspace guards, deploy storage, replay liveness, the shard half of runtime isolation | rholang `reduce.rs`, rspace `replay_rspace.rs`, and the runtime manager, interpreter util, and replay runtime that live under the casper crate today | Every medium that runs Rholang, including RGB where rho names are sealed |
| DAG substrate | carrier index, block admission, deploy occurrence, the block heap half of runtime isolation | block-storage DAG storage and the carrier index | Casper, Casanova, and Cordial Miners. Not RGB |
| Casper medium | fork choice, finalized floor, slashing, deploy recovery, deploy lifecycle, recovery leader, and the Casper theory docs | snapshot, block creator, validation dispatcher, finality, the two mergers, deploy chain index, validate, genesis deploys | Casper only |
| RGB machine B and seal medium | None in this tree. Schema validity, single seal close, and consignment validation start in the RGB repository | None in this tree | RGB only |
| Infrastructure | soak disk protection, storage budget | driver evidence | Every medium the soak exercises |

**The keystone.** The Rocq merge algebra proves the commutation claim in machine form. Its theorems cover pointwise commutativity, associativity, and idempotence of effect-map merge, deterministic channel netting, conflict soundness, and a keep-one order. Each medium repository cites those theorems by pinned commit and does not carry a copy.

**Follow-ups the split depends on**

- **Interface crate.** The `MultiParentCasper` trait is defined inside the casper crate. The node therefore names its boundary through the medium it should be neutral to. A neutral interface crate comes first. Section 11 records the open question of a thinner `OrderingMedium` trait for RGB.
- **Rocq coverage gap.** The formal gate rebuilds only slashing, fork choice, and rspace guards. The merge algebra, finalized floor, and runtime isolation proofs ship as committed build outputs, and CI does not recheck them. The keystone needs a CI rebuild before anything cites it by pin.
- **Execution glue in the medium crate.** Three Rholang runtime files under the casper crate are machine A code. They move with the interface work.
- **Runtime isolation splits.** `ShardRuntimeIsolation` is machine A. `BlockHeapLifecycle` is substrate. The area is cut in two at the move.
- **Soak scenarios per medium.** The merge-recovery soak exercises Casper. The driver and its disk models are shared, but each medium needs its own scenarios.

The soak work log records the same split and its history: [`docs/work-logs/task-soak-disk-hygiene-parsimonious-2026-09-09.md`](../work-logs/task-soak-disk-hygiene-parsimonious-2026-09-09.md).

---

## 10. Maturity board

The tiers come from the Smart Assets standard. `coming_soon` is the only non-interactive tier.

| Component | Tier | Backend state | Notes |
| --- | --- | --- | --- |
| RSpace and Rholang execution | `demo` | Shard and standalone runtime in `f1r3node-rust` | Soak-gated with Casper. Not unmarked Production |
| CBC Casper engine | `demo` | Multi-validator shards, the 60 h stability soak, and the test net | Trait `MultiParentCasper`. Formal stack under `formal/` |
| F1r3fly-RGB SM with BTC and LN seals | `alpha` | Partial real seal media, with a client-side stash | Private tree. Publish path to be decided |
| Casanova adapter | `coming_soon` | Specification only (Pyrofex, arXiv:1812.02232) | Implementation work in progress. No crate in `f1r3node-rust` |
| Cordial Miners adapter | `coming_soon` | Paper only | Parallel repository later. Cited only in this tree |

Promotion rules for an adapter:

1. Promote to `prototype` only after an in-memory or harness engine drives `ReplayRSpace` through the same post-state check Casper uses.
2. Promote to `alpha` when a shard or seal network is in the loop.
3. Promote to `demo` when a testnet or sandbox backend exists.
4. Production requires a production backend and no designation badge.

---

## 11. Implementation map

| Path | Role |
| --- | --- |
| `casper/` | Shipping CBC Casper: estimator, clique oracle, finalizer, equivocation, slashing, block creator |
| [`docs/casper/CONSENSUS_PROTOCOL.md`](../casper/CONSENSUS_PROTOCOL.md) | The pipeline and the consensus-specific versus agnostic split |
| [`docs/Glossary.md`](../Glossary.md) | Load-bearing terms: merge scope, floor, content ordering, loss-aware adjudication |
| `rholang/`, `rspace++/` | Machine A |
| `formal/` | TLA+, Rocq, and Kani on consensus-critical Casper surfaces, grouped by the checks in section 9 |
| [`docs/formal-verification.md`](../formal-verification.md) | The guide to every formal area and its gate |
| F1r3fly-RGB, private tree | Machine B with BTC and LN seals (`alpha`) |
| [arXiv:1812.02232](https://arxiv.org/abs/1812.02232) | Casanova specification |
| [arXiv:2205.09174](https://arxiv.org/abs/2205.09174) | Cordial Miners specification |

---

## 12. Non-goals and open work

**Non-goals for this note**

- A Production claim for Casper or RGB.
- Cordial or Casanova as in-tree features.
- Equating F1r3fly Casper with RChain Casper or Ethereum Casper FFG.
- RGB validation inside the clique oracle.
- A global total order of commuting deploys.

**Open work**

- Publish or submodule F1r3fly-RGB next to `f1r3node-rust`. Document the exact commitment scheme: which outputs, tapret or opret, and the LN commitment.
- Specify the name-to-seal binding. Which rho names are sealed, who constructs the consignment, and how a Casper-finalized deploy may *emit* a seal close without making Bitcoin a Casper parent.
- Casanova: the first vertical slice is conflict isolation plus an FTM lock that feeds the rejected-deploy buffer Casper already has. It is not a second runtime.
- Cordial: when the parallel repository exists, map the blocklace τ onto content ordering and merge, not onto RSpace reduce.
- Decide whether `Arc<dyn MultiParentCasper>` grows a thinner `OrderingMedium` trait, so that RGB is not forced into a DAG-parent API.
- Close the two verification gaps in section 9: the neutral interface crate and the Rocq CI rebuild of the merge algebra.

---

## 13. One-sentence summary

F1r3fly runs parallel state machines, RSpace and RGB, whose transitions commute unless names or seals collide. Casper, Bitcoin and Lightning seals, Casanova, and later a Cordial blocklace are interchangeable uniqueness media for those collisions, not the definition of the machines.
