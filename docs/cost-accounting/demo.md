# Cost-Accounting Demo (`examples/cost_accounting_demo.rho`)

A **token-gated buyer–seller ecosystem** in one Rholang file, runnable by
`rholang-cli`. It exercises **every** cost-accounting surface feature in a motivated
supply-chain story, layered over a **guarded, transactional dual-resource ledger**
(money + inventory), while fully embracing Rholang's parallelism — with a companion
**rho-calculus correctness proof** ([`demo-proof.md`](./demo-proof.md)).

## Run it

```bash
# from the f1r3node worktree root:
cargo run -p rholang --bin rholang-cli --                        examples/cost_accounting_demo.rho  # events + final ledgers (storage dump)
cargo run -p rholang --bin rholang-cli -- --quiet               examples/cost_accounting_demo.rho  # console lines only
cargo run -p rholang --bin rholang-cli -- --unmatched-sends-only examples/cost_accounting_demo.rho  # resting sends, incl. the ring-fenced new-bound fuel token
```

(The worktree carries the dev `[patch]` flip to the local cost-syntax parser + the
cost normalizer, so the CLI parses `{% P %}[s]` / `s :: ()` / `(*)` / `::` / `-o` /
`# P`, lowers them via §8, and reduces the result. The surface syntax is Greg's
`cost-accounted-rho.tex@fdf089e §app:concrete`.)

## The story

A supply chain. **Factories** produce widgets; **carriers** ship them to a
**warehouse**; **sellers** wholesale-buy them into **stores**; **buyers** retail-buy
them **home**; buyers may **return** a widget for resale. Two resources flow in
opposite directions and are both conserved, and **every** operation needs a fuel token.
Sellers also run **order desks**, where *taking an order off the queue is itself
fuel-metered* — the per-clause signed bind meters the **receive**, not just the send.

**Three distinct quantities — do not conflate them:**

| Quantity | What it is | Channel kind | Conserved? |
|---|---|---|---|
| **Fuel token** | *authorization* to act; a stack's depth is an operation **budget** | unforgeable `Σ⟦s⟧` | no — spent once per op |
| **Money** | account balances | `@"…_cash"` cells | **yes** — total constant (410) |
| **Inventory** | widget counts | `@"…_stk"`/`_out`/`_home` cells | **yes, after production** (= 67 + produced) |

A **purchase is an atomic swap over both resources at once**: money flows buyer→seller
**and** goods flow store→home in a single all-or-nothing N-ary `JOIN` — you never pay
without receiving goods, nor receive goods without paying.

## Every cost-accounting feature, mapped

| Feature | Where it shows up |
|---|---|
| **R1** single ground sig | retail buys `[ada]`, flash buys `[faeSig]`/`[gusSig]` |
| **R2** compound `s1 (*) s2` (signature *multiplication*) | shipping `[fab1 (*) carrier]` — factory **and** carrier must sign |
| **R3** combined-cell token + splitter | wholesale `[sueMkt1 (*) sueMkt2]` + `sueMkt1 (*) sueMkt2 :: ()` |
| **`# P`** section signature | factory production `[# "Fab1"]` (authorizes via its own identity) |
| **`::` multi-token budget** (depletes) | Ada `ada :: ada :: ()` funds 2 buys; Ben's budget-2 meters out his 3rd |
| **`-o` lollipop** delegation | Cy delegates shopping `[cyPrincipal -o cyAgent]` (a signed `for`-continuation) |
| **`new`-bound sig** ring-fencing | Di's funds `new diSig in { diSig :: () }` — a thief signing a *free* `diSig` is blocked |
| **per-clause signed bind `{% y<-x %}[s]`** (Axis-C join) | the metered **order desks** — meters a *rendezvous*, not a process: Sue's intake `[sueDesk]`, the bid/ask deal-match join `[bidSig] & [askSig]`, Zed's unfunded desk (⊥) |
| **N-ary `&` join** | the 4-cell atomic swap (purchase/wholesale/return) |
| **arithmetic `*`** (multiplication) | `price * qty` on every swap; the guard-fail uses `100 * 5 = 500` |
| **guard + `true`/`false`/⊥** | every swap answers on a response channel; three rejection modes |
| **atomic transaction** | the swap (money + goods together) |
| **mutex lock** | the multi-step warehouse→store restock critical section |
| **embraced contention** | two buyers race for the last flash-sale unit |

## Expected output

`--quiet` prints the console announcements (the **set** is stable every run; order
varies). The authorized operations announce and audit `true`:

```
("PRODUCED", 10, "into", "Fab1_out")            ("PRODUCED", 6, "into", "Fab2_out")
("SHIP-OK", "Fab1_out", "->", "Whse_stk", 10)   ("SHIP-OK", "Fab2_out", "->", "Whse_stk", 6)
("SWAP-OK", "Sue_cash", "paid", 16, "to", "Whse_cash", "for", 8, "units")   # wholesale
("SWAP-OK", "Sam_cash", "paid", 12, "to", "Whse_cash", "for", 6, "units")
("SWAP-OK", "Ada_cash", "paid", 15, "to", "Sue_cash", "for", 3, "units")    # Ada buy 1
("SWAP-OK", "Ada_cash", "paid", 10, "to", "Sue_cash", "for", 2, "units")    # Ada buy 2 (budget depleted)
("SWAP-OK", "Ben_cash", "paid", 4, "to", "Sam_cash", "for", 1, "units")     # Ben micro-buy  x2 only
("SWAP-OK", "Cy_cash",  "paid", 10, "to", "Sue_cash", "for", 2, "units")    # lollipop delegate
("SWAP-OK", "Gus_cash", "paid", 20, "to", "Sue_cash", "for", 1, "units")    # flash-sale winner
("SWAP-OK", "Sue_cash", "paid", 4,  "to", "Ada_cash", "for", 1, "units")    # return / refund
("RESTOCK-CS: warehouse holds", 34, "moving", 3)  ("RESTOCK done", 3)        # mutex, serialized
("SWAP-DENIED", "Fae_cash", "->", "Sue_cash", "(funds or stock short)")     # flash-sale loser
("SWAP-DENIED", "Ben_cash", "->", "Sue_cash", "(funds or stock short)")     # Ben's $500 buy
("AUDIT", ("Ben BIG buy 5 @100 (must be denied)", false))
("AUDIT", ("Sue desk intake", "ada: 3 widgets"))                            # per-clause bind (funded)
("AUDIT", ("deal matched", "Cy bids 50", "Sue asks 48"))                    # two-clause Axis-C join
```

The demo shows **distinct rejection modes**:

* **`false` — funds/stock short.** Ben's `100 * 5 = 500` buy (deterministically denied);
  the flash-sale loser (out of stock). The caller's audit prints `false`.
* **⊥ — fuel-denied (no response at all).** Ben's **third** identical micro-buy (his
  budget of 2 is spent), the **ring-fenced** drain of Di's wallet (a thief signs a
  *free* `diSig`, disjoint from her `new`-bound `Σ⟦diSig⟧`), and **Eve's** unfunded
  theft. The gate never fires, so the contract is never invoked and the response channel
  is never answered: these print **no** `AUDIT` line at all (only two `Ben micro-buy`
  audits appear, not three). The ring-fenced `new`-bound fuel token also remains
  **unconsumed** in the storage dump — appearing as a bare `Unforgeable(0x…)!(Nil)` send
  under `--unmatched-sends-only` — the direct evidence that no free-`diSig` gate ever
  drained Di's wallet.
* **⊥ — intake-unfunded (the receive side).** **Zed's** order desk
  `for( {% @ord <- @"Zed_orders" %}[ zedDesk ] ){…}` has a message waiting on its queue
  but no `zedDesk` fuel, so the *rendezvous itself* is metered out: the per-clause gate
  parks and the order is never intaken (no `AUDIT` line). The `@"Zed_orders"` message
  rests **unconsumed** in the storage dump — proof that a per-clause signed bind meters
  the receive, not just the send. (Contrast Sue's funded desk and the funded bid/ask
  deal-match join, which both fire.)

The full run also prints `Storage Contents`, where the **final ledgers** appear:

```
CASH:  Ada 79  Ben 52  Cy 40  Di 40  Sue 85  Sam 46  Whse 28  Fae 30  Gus 10     (Σ = 410)
STOCK: Whse_stk 20  Sue_stk 22  Sam_stk 30  Flash_stk 0  Ada_home 6  Ben_home 2
       Cy_home 2  Gus_home 1  Fae_home 0  Fab1_out 0  Fab2_out 0                  (Σ = 83)
```

* **Money total = 410** and **widget total = 83 (= 67 opening + 16 produced)** — both
  conserved, every run. No cell is ever negative (the guards).
* **Di keeps her 40** (ring-fenced), the Vault-style drain never runs.
* (Each cell appears twice in the raw dump: once in the pre-reduction `Evaluating
  rhocli:` echo, once in `Storage Contents:` — the latter is the final value.)

## Embracing parallelism — and what's deterministic

Everything is one flat parallel composition; there is **no sequential execution**. The
only ordering is fuel-gated rendezvous, the atomic JOINs, and the restock lock. Opening
inventory/cash cover every authorized operation in **every** interleaving, so the
**safety invariants hold every run** — money 410, widgets 83, nothing negative, exactly
one flash-sale winner, no partial swaps.

The last-unit flash sale is a **genuine race**: `Flash_stk = 1`, two buyers each want 1,
so exactly **one** wins and the other is correctly told "out of stock" (no double-spend).
The rho calculus permits **either** winner; this particular reducer happens to resolve
the race deterministically (Gus, here). Only print order is visibly schedule-dependent.

That structure — a deterministic, conserved core plus one characterized contention pool —
is a theorem: see [`demo-proof.md`](./demo-proof.md) §4–§6 (conservation + non-negativity),
§8 (atomic-swap atomicity), §9 (mutual exclusion), §11 (the two-outcome contention space).

## Correctness proof

[`demo-proof.md`](./demo-proof.md) proves, in the rho calculus, the **invariant-based**
correctness of a deliberately non-confluent system: cell-singleton, **money
conservation**, **inventory conservation modulo production**, **non-negativity** of both
ledgers, **fuel-budget enforcement** (the depth-k stack lemmas), **atomic-swap
atomicity** (all-or-nothing over money *and* goods), **mutual exclusion** at the lock,
**response-faithfulness** (true/false/⊥), **strong normalization**, and a precise
**contention outcome-space** characterization (the reachable normal forms are exactly the
invariant-valid family, with multiplicity only from the flash-sale pool) — plus agreement
with the source rules R1–R3 + lollipop + the **Axis-C per-clause join** (the order desks,
conservation-neutral) and Graded Adequacy ([GSLT] Thm 1). The new
interpreter mechanics are also covered as Rust reduction tests in
`rholang/tests/accounting/cost_accounting_reduction_spec.rs` (same-signature budgets;
return-channel forwarding through a fuel gate).
