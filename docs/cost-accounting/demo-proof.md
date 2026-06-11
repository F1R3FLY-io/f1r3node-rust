# Rho-Calculus Correctness Proof — `examples/cost_accounting_demo.rho`

The demo is a **token-gated buyer–seller ecosystem**: factories produce widgets,
carriers ship them to a warehouse, sellers wholesale-buy them into stores, buyers
retail-buy them home, and buyers may return widgets for resale. Two resources flow
in opposite directions and are tracked as integer cells — **money** (`*_cash`) and
**inventory** (`*_stk`, `*_out`, `*_home`) — and **every** operation is gated by a
cost-accounting **fuel token**.

Because the demo deliberately **embraces contention** (two buyers race for the last
flash-sale unit), it is **not confluent**: there is no unique normal form. We
therefore prove correctness in the **safety-invariant + outcome-space** sense:

* a family of **invariants holds in every reachable state** — cell-singleton (§3),
  **money conservation** (§4), **inventory conservation modulo production** (§5),
  **non-negativity** of both ledgers (§6), **fuel-budget enforcement** (§7; incl. the
  per-clause-bind / Axis-C rendezvous metering at the order desks),
  **atomic-swap atomicity** (§8), **mutual exclusion** at the restock lock (§9),
  and **response-faithfulness** (§10);
* the system **strongly normalizes** (§12);
* the set of reachable **normal forms** is exactly a **characterized valid family**
  (§11): multiplicity arises *only* from the one contention pool, and every member
  conserves value, stays non-negative, and answers faithfully.

This **reverses** the prior payment-ledger proof's "no overdraft guard" choice
(old §7): we add guards, gaining the non-negativity safety property and a genuine
nondeterministic-but-correct race, at the cost of confluence.

> **Methodological stance — "no unique normal form, yet provably correct."**
> (i) The safety invariants are ∀-reachable-state properties, closed under `→`, so
> they hold under *every* schedule. (ii) The outcome set is pinned to an enumerated
> valid family (§11) via strong normalization + a serialization analysis of the one
> contended cell. (iii) Determinism is recovered everywhere except that cell: all
> non-contending steps commute, and an atomic bundle would collapse even the pool.

**Citations.** `[CAR]` = `../../../publications/cost-accounting/cost-accounted-rho.tex`
(§3.2 rules R1–R5; §3.3 N-ary join `eq:join-rule`, atomicity prose, `eq:join-J1/J2`;
§3.5 sugars `def:sugar-uniform`/`def:sugar-lollipop`; App. A Verification Sketch
`app:verification`, `prop:subst-commute`). `[GSLT]` =
`../../../publications/cost-accounting-as-monad/continued-gslt-cost-v2.tex`
(§5 `thm:adequacy`, Graded Adequacy).

> **Rigor flag R-0 (layering).** The `[CAR]` calculus is the pure-rho fragment plus
> signing; it does **not** natively define `match`/`if`, ground arithmetic, or
> boolean evaluation. The demo's guards (`pm >= price * qty and ss >= qty`), its
> arithmetic (`pm - price*qty`, `ds + qty`), and its `match (bool){…}` live at the
> **pure-Rholang target level** — in the program the CLI actually reduces, *after*
> the §8 lowering. §1.2 states them as standard Rholang reduction rules layered on
> the `[CAR]` fragment; `[CAR]`/`[GSLT]` citations are confined to the
> fuel/COMM/JOIN/authorization layer where they apply.

---

## 1. Preliminaries

### 1.1 The rho fragment

We use `0`, send `x!(Q)`, input `for(y<-x){P}`, the **N-ary join**
`for(y₁<-x₁ & … & y_N<-x_N){P}`, the **persistent** input `for(y<=x){P}`,
composition `P|Q`, dereference `*x`, `new x in P`, names `@P`, and unforgeable
`new`-bound names.

**Structural congruence `≡`** ([CAR] §3.3): the least congruence making `(P,|,0)` a
commutative monoid, with α-equivalence, `new`-scope extrusion, and **REFL**
`*@P ≡ P` (hence `@*x ≡ x` for names — the round-trip used to forward response
channels; validated operationally by the test
`fuel_gate_forwards_outer_new_bound_return_channel`).

**Reduction `→` (fuel/authorization layer — cite [CAR]).**
```
(COMM)   for(y <- x){P}  |  x!(Q)                          →  P{@Q/y}
(COMM@)  for(@y <- x){P} |  x!(Q)                          →  P{Q/y}
(JOIN_N) for(y₁<-x₁ & … & y_N<-x_N){P} | x₁!(Q₁)|…|x_N!(Q_N) →  P{@Q₁/y₁,…,@Q_N/y_N}   ([CAR] §3.3, eq:join-rule)
(REPL)   for(y <= x){P}  |  x!(Q)                          →  P{@Q/y}  |  for(y <= x){P}
(CONG)   closure under `|` and `≡`.
```
**(JOIN_N) is atomic** ([CAR] §3.3, `rem:db-atomicity`): it fires only when *all* N
sends are simultaneously present, consuming all N in one step. **No rule performs a
partial join** — there is no state in which a strict nonempty subset of `{x₁..x_N}`
has been consumed by a given join while the rest remain. R1–R5 are the N=1 cases.

### 1.2 Target-level rules (cite the Rholang/RSpaceII semantics, NOT [CAR]; R-0)

* **`match` on a ground boolean.** `match true {true=>P; false=>Q} → P` and
  `match false {…} → Q`. The scrutinee is evaluated to a `GBool` *before* the case
  is selected (the reducer's `eval_match`). **Fact (match determinism):** for a
  closed ground boolean scrutinee exactly one arm is selected; the step is local and
  commutes with every redex on disjoint subterms.
* **Ground evaluation.** `a - b`, `a + b`, `a * b` evaluate to a unique integer;
  `a >= b` and `b₁ and b₂` to a unique boolean. **Fact (eval determinism):**
  evaluation of a closed ground arithmetic/relational/boolean expression is a
  deterministic, terminating function; these steps commute with all parallel redexes.
  In all ledger reasoning we treat `a-b`, `a+b`, `a*b`, `a>=b` as the mathematical
  functions on `ℤ`/`𝔹`.

### 1.3 Lowering facts (verified transpiler; `cost_accounting_reduction_spec.rs`)

* (L1) A signature `s` lowers to a content-addressed unforgeable channel
  `Σ⟦s⟧`. **(L1′) Injectivity:** distinct signatures give distinct channels
  (normalizer test `sigma_is_injective_on_distinct_signatures`); a compound
  `Σ⟦s₁ ∘ s₂⟧ = @( *Σ⟦s₁⟧ | *Σ⟦s₂⟧ )` (AC, key-sorted; surface `s₁ (*) s₂`); a
  hash `Σ⟦#P⟧` is keyed by a distinct domain from ground sigs.
* (L2) A token stack lowers by `K`: `K⟦()⟧ = Nil`, `K⟦s::S⟧ = Σ⟦s⟧!(K⟦S⟧)` — a
  chain of **non-persistent** sends. A depth-k same-signature stack is
  `Σ⟦s⟧!(Σ⟦s⟧!(…!(Nil)))`. (A bare stack is itself a process — there is no
  `purse(...)` wrapper.)
* (L3) A signed term `{P}_s` lowers to the **non-persistent** fuel gate
  `for(t<-Σ⟦s⟧){ *t | P⟦P⟧ }`. Consuming a token binds `t` to the payload and `*t`
  re-launches it (releasing the chain tail), then `P⟦P⟧` runs. **Both the
  token-send and the gate are non-persistent** — load-bearing for §7.
* (L4) **Binding-sensitive `Σ` (ring-fencing).** Resolution of a ground sig `s`
  consults the gate's enclosing binders. A **free** `s` is content-addressed by
  **spelling** over `DOMAIN_GROUND` (the same `s` everywhere ⇒ one channel). A
  **`new`-bound** `s` (an enclosing `new s`) is content-addressed by its
  **binder's identity** (source span) over `DOMAIN_BOUND`, so
  `Σ⟦bound s⟧ ≠ Σ⟦free s⟧` and two distinct `new s` binders give distinct
  channels. Ring-fencing: a token minted under `new s` is unreachable to any gate
  signing a free (or differently-bound) `s`. This **replaces** the located purse
  `purse(I,…)`.
* (L5) The **lollipop** `[s₁ -o s₂]` desugars ([CAR] `def:sugar-lollipop`) so `s₁`
  funds the rendezvous and the continuation is re-signed with `s₂`.
* (L6) A **per-clause signed bind** `for( {% y<-x %}[s] & … ){P}` lowers
  (`lower_signed_join`) by stripping each signed clause to a plain linear bind and
  gating the recovered `for` with one fuel gate per clause atom:
  `for( {% y<-x %}[s] ){P} = for(t<-Σ⟦s⟧){ *t | for(y<-x){P} }`. So it meters the
  **rendezvous** (one token per clause) rather than a process, and `P` is **not**
  re-signed (the distinction from a signed term, L3). An N-clause join gates by the
  product `Σ⟦s₁⟧,…,Σ⟦sₖ⟧` — the comm fires only once every clause's token is consumed.

The outer-`new`-bound response channel `*ret` forwarded inside `P⟦P⟧` receives the
correct de-Bruijn offset across the gate binder (validated:
`fuel_gate_forwards_outer_new_bound_return_channel`; and across the *nested* lollipop
gates by the demo's Cy scene, which settles).

### 1.4 The observable (for a nondeterministic system)

At a state `W` (and, for safety, pointwise at every reachable state):

1. **`SETTLED(W)`** — the multiset of `@"display"` and `@"audit"` announcements
   delivered to the persistent consoles `for(@l<=@"display"){stdout!(l)}` /
   `for(@l<=@"audit"){stdout!(("AUDIT",l))}`. A multiset, because under contention a
   `SWAP-OK` may be a `SWAP-DENIED` in another schedule.
2. **`RESPONSE_c ∈ {true,false,⊥}`** for each caller `c` holding `for(@ok<-ret_c){…}`:
   the boolean received on its private `ret_c`, or **⊥** if `ret_c` is never answered.
3. **`CASH : Account→ℤ`** and **`STOCK : Location→ℤ`** — the balance/count carried by
   each ledger cell at the normal form (the `Storage Contents` dump).

> **Remark 1.1 (NOT confluent — reverses the prior §7).** The prior payment-ledger
> proof omitted the overdraft guard so balance updates were pure additive deltas that
> commuted, giving a unique normal form via Newman. This demo **adds** the guard
> `match (pm >= price*qty and ss >= qty){…}`. Where two transfers contend for a scarce
> cell, the read value differs by schedule, so the scrutinee differs, so distinct
> interleavings reach structurally distinct normal forms (different `SETTLED`,
> `RESPONSE`, `STOCK`). **Additive commutativity fails by design.** We drop the
> confluence/Newman route to a unique observable and prove safety + an outcome-space
> characterization (§11). This is a deliberate, documented trade: contention-
> correctness over confluence.

---

## 2. The program and its lowering

### 2.1 Infrastructure (unmetered; persistent)

```
SWAP   = for(@payer,@payee,@src,@dst,@price,@qty,@ret <= @"swap"){
           for(@pm<-@payer & @ym<-@payee & @ss<-@src & @ds<-@dst){          // atomic 4-cell JOIN
             match (pm >= price*qty and ss >= qty){
               true  => @payer!(pm-price*qty) | @payee!(ym+price*qty)
                      | @src!(ss-qty) | @dst!(ds+qty) | @ret!(true) | display(OK)
               false => @payer!(pm) | @payee!(ym) | @src!(ss) | @dst!(ds)  // re-send UNCHANGED
                      | @ret!(false) | display(DENIED) } } }
MOVE   = for(@src,@dst,@qty,@ret <= @"move"){
           for(@ss<-@src & @ds<-@dst){
             match (ss >= qty){ true=>@src!(ss-qty)|@dst!(ds+qty)|@ret!(true)|disp
                                false=>@src!(ss)|@dst!(ds)|@ret!(false)|disp } } }
PRODUCE= for(@bin,@qty,@ret <= @"produce"){ for(@n<-@bin){ @bin!(n+qty)|@ret!(true)|disp } }
RESTOCK= for(@amt,@ret <= @"restock"){
           for(_<-@"restockLock"){                                          // ACQUIRE
             for(@wh<-@"Whse_stk"){
               display(("RESTOCK-CS",wh,amt)) |                             // announce mid-CS
               match (wh>=amt){ true=>@"Whse_stk"!(wh-amt) | for(@s<-@"Sam_stk"){@"Sam_stk"!(s+amt)}
                                     |@ret!(true)|disp|@"restockLock"!(Nil) // RELEASE
                                false=>@"Whse_stk"!(wh)|@ret!(false)|disp|@"restockLock"!(Nil) } } } }
```
Here `@payer` denotes the channel quoting the *process* bound to `payer`; after a
request sends the string `"Ada_cash"`, `@payer = @"Ada_cash"`, the cash cell. **All
four of `payer,payee,src,dst` are distinct strings in every scene** (checked below),
so the 4-cell join reads four distinct cells.

The lock token `@"restockLock"!(Nil)` is present initially (one token).

### 2.2 Initial state

```
CASH₀ : Ada 100, Ben 60, Cy 50, Di 40, Sue 50, Sam 50, Whse 0, Fae 30, Gus 30   (Σ = 410)
STOCK₀: Fab1_out 0, Fab2_out 0, Whse_stk 24, Sue_stk 20, Sam_stk 20, Flash_stk 1,
        Ada_home 2, Ben_home 0, Cy_home 0, Fae_home 0, Gus_home 0                (Σ = 67)
```

### 2.3 Metered requests and their fuel (the scene set)

Each request lowers by (L1–L6). Writing `A=Σ⟦ada⟧`, `B=Σ⟦ben⟧`, etc.:

| Scene | request (gist) | sig / fuel | feature |
|---|---|---|---|
| Prod1,2 | `produce(Fab_i_out, q)` | `[# "Fab_i"]`, one token each | **#P** section sig |
| Ship1,2 | `move(Fab_i_out, Whse_stk, q)` | `[fab_i (*) carrier]`, `fab_i :: () \| carrier :: ()` | **R2 compound `(*)`** |
| WS1,2 | `swap(Seller, Whse, Whse_stk, Store, 2, q)` | `[m1 (*) m2]`, `m1 (*) m2 :: ()` | **R3 combined + splitter** |
| Ada1,2 | `swap(Ada,Sue,Sue_stk,Ada_home,5,{3,2})` | `[ada]`, `ada :: ada :: ()` | **R1 + depth-2 budget** |
| Ben×3 | `swap(Ben,Sam,Sam_stk,Ben_home,4,1)` ×3 identical | `[ben]`, `ben :: ben :: ()` | **budget over-quota** |
| Cy | `for(@q<-@"cyOrder"){swap(Cy,Sue,Sue_stk,Cy_home,5,q)}` | `[cyP -o cyA]` (signed `for`-cont.), `cyP :: () \| cyA :: ()`, `@"cyOrder"!(2)` | **lollipop `-o`** |
| Di | `swap(Di,Whse,Sue_stk,Cy_home,5,2)` | `[diSig]` (thief signs **free** `diSig`); fuel `new diSig in { diSig :: () }` | **ring-fenced (⊥)** |
| Fae,Gus | `swap({Fae,Gus},Sue,Flash_stk,{Fae,Gus}_home,20,1)` | `[faeSig]`,`[gusSig]`, one token each | **contention** |
| BenBig | `swap(Ben,Sue,Sue_stk,Ben_home,100,5)` | `[benBig]`, one token | **guard-fail (false)** |
| Eve | `swap(Sue,Whse,Sue_stk,Cy_home,5,3)` | `[eve]`, **no token** | **metering (⊥)** |
| Return | `swap(Sue,Ada,Ada_home,Sue_stk,4,1)` | `[adaRet]`, one token | **reverse swap** |
| Restock1,2 | `restock(3)` ×2 | `[restock_i]`, one token each | **mutex** |
| SueDesk | `for( {% @ord<-@"Sue_orders" %}[sueDesk] ){…}`, `@"Sue_orders"!(…)` | `[sueDesk]`, `sueDesk :: ()` | **per-clause bind** |
| Deal | `for( {% @bid<-@"bids" %}[bidSig] & {% @ask<-@"asks" %}[askSig] ){…}`, `@"bids"!(…)\|@"asks"!(…)` | `bidSig :: () \| askSig :: ()` | **Axis-C join** |
| ZedDesk | `for( {% @ord<-@"Zed_orders" %}[zedDesk] ){…}`, `@"Zed_orders"!(…)` | `[zedDesk]`, **no token** | **per-clause metering (⊥)** |

> **Conservation-neutrality of the desks.** SueDesk, Deal, and ZedDesk read only the
> dedicated queue channels `@"Sue_orders"`, `@"bids"`, `@"asks"`, `@"Zed_orders"` — all
> **disjoint** from the cash cells `@"·_cash"` and stock cells `@"·_stk"/·_out/·_home`,
> and their bodies emit only `@"audit"` announcements. They neither read nor write any
> ledger cell, so the conservation and non-negativity theorems (§4–§6) are **unaffected**
> by them: the metered-intake scenes are an orthogonal feature demonstration.

> **Fact 1 (distinct fuel channels).** All the `Σ⟦·⟧` of distinct signatures above are
> pairwise distinct (L1′), and `Σ⟦bound diSig⟧ ≠ Σ⟦free diSig⟧` (L4). *Proof.* Ground
> sigs are content-addressed by distinct names; `# "Fab_i"` is hash-domain; compounds
> are multi-`GPrivate`; the `new`-bound `diSig` channel is keyed by its binder over
> `DOMAIN_BOUND`, disjoint from the free `diSig` channel over `DOMAIN_GROUND`. ∎

---

## 3. Cell-singleton invariant

> **Lemma 3.1.** In every reachable state each ledger cell (`*_cash`, `*_stk`,
> `*_out`, `*_home`) carries **exactly one** token, *except* transiently while a join
> consuming it is mid-continuation (owned by §8).

*Proof.* Induction on reduction length; IH = the invariant. **Base:** `CASH₀`/`STOCK₀`
send each cell exactly once. **Step — enumerate every rule touching a ledger cell:**

* **SWAP join fires.** Its 4-cell `(JOIN_4)` consumes one token from each of
  `payer,payee,src,dst` (one each, by IH; four distinct cells by §2.1). The
  continuation is a `match` whose **both** arms re-send exactly one token to each:
  `true` → `@payer!(pm-…)|@payee!(ym+…)|@src!(ss-…)|@dst!(ds+…)`; `false` →
  `@payer!(pm)|@payee!(ym)|@src!(ss)|@dst!(ds)`. By match-determinism exactly one arm
  runs; either re-sends one token per cell. Count restored to one.
* **MOVE join fires.** 2-cell consume; both arms re-send one each. ✓
* **PRODUCE fires.** Consumes `@bin` (one), re-sends `@bin!(n+qty)` (one). ✓
* **RESTOCK fires.** Reads `@"Whse_stk"` (one), and in the `true` arm
  `for(@s<-@"Sam_stk"){@"Sam_stk"!(s+amt)}` consumes and re-sends `@"Sam_stk"` (one);
  both arms re-send `@"Whse_stk"` exactly once. ✓
* **No other rule** sends to or receives from a ledger cell: the consoles act on
  `@"display"`/`@"audit"`, the lock on `@"restockLock"`, fuel on `Σ⟦·⟧`, responses on
  private `ret_c`, orders on `@"cyOrder"` — all distinct from the ledger channels
  (Fact 1 + the cells are fixed strings). ∎

> **Remark (transient absence).** Between a join's consume and its arm's re-send the
> joined cells are absent; §8 shows no other step can read them in that window.

---

## 4. Money conservation

> **Lemma 4.1.** `T_cash = Σ_acct CASH(acct) = 410` in **every** reachable state.

*Proof.* Induction on `→`; IH = "`T_cash = 410` ∧ Lemma 3.1." **Base:** `Σ CASH₀ = 410`.
**Step:** the only cash-writing rules are the SWAP arms (MOVE/PRODUCE/RESTOCK touch no
`*_cash` cell; consoles/fuel/locks/responses touch no cell):
* **SWAP true:** `payer ↦ pm - price*qty`, `payee ↦ ym + price*qty`; net `0`.
* **SWAP false:** re-sends `pm`, `ym` unchanged; net `0`.
No rule changes `T_cash`. ∎ *(The five SWAP scenes that move cash — Ada×2, Ben-micro×2,
Cy, the contention winner, the two wholesales, the return — each net zero; the
denied/parked ones move nothing.)*

---

## 5. Inventory conservation modulo production

> **Lemma 5.1.** Let `produced(W)` = total widgets minted by PRODUCE up to `W`. Then
> `T_stock(W) = Σ_loc STOCK(loc) = 67 + produced(W)` in every reachable state. At any
> normal form `produced = 16` (both factories fire once, §7), so `T_stock = 83`.

*Proof.* Induction on `→`; IH = "`T_stock = 67 + produced` ∧ Lemma 3.1." **Base:**
`Σ STOCK₀ = 67`, `produced = 0`. **Step — stock-writing rules:**
* **PRODUCE:** `bin ↦ n + qty`; `T_stock += qty` and `produced += qty`; invariant
  preserved (the *only* rule that raises `produced`).
* **SWAP true:** `src ↦ ss-qty`, `dst ↦ ds+qty`; net `0`.
* **SWAP false / MOVE false:** unchanged; net `0`.
* **MOVE true:** `src ↦ ss-qty`, `dst ↦ ds+qty`; net `0`.
* **RESTOCK true:** `Whse_stk ↦ wh-amt`, `Sam_stk ↦ s+amt`; net `0`. **false:** net `0`.
No rule other than PRODUCE changes `T_stock - produced`. ∎

---

## 6. Non-negativity of both ledgers

> **Lemma 6.1.** `CASH(acct) ≥ 0` and `STOCK(loc) ≥ 0` in every reachable state.

*Proof.* Induction on `→`; IH = "all balances/counts ≥ 0 ∧ Lemma 3.1." **Base:** all of
`CASH₀`, `STOCK₀` are ≥ 0. **Step — enumerate every write site and show each writes ≥ 0:**

* **SWAP true** (reached only when the scrutinee `pm >= price*qty and ss >= qty` is
  `true`, by match-determinism): writes `pm - price*qty ≥ 0` (since `pm ≥ price*qty`)
  and `ss - qty ≥ 0` (since `ss ≥ qty`); writes `ym + price*qty ≥ 0` and `ds + qty ≥ 0`
  (sum of IH-≥0 and a non-negative literal — all `price,qty ≥ 0` are non-negative
  ground constants in the program). ✓
* **SWAP false / MOVE false / RESTOCK false:** re-send IH-≥0 values unchanged. ✓
* **MOVE true:** guarded by `ss ≥ qty` ⇒ `ss-qty ≥ 0`; `ds+qty ≥ 0`. ✓
* **PRODUCE:** `n + qty ≥ 0`. ✓
* **RESTOCK true:** guarded by `wh ≥ amt` ⇒ `wh-amt ≥ 0`; `s+amt ≥ 0`. ✓
No other write site exists (§3). ∎

> **Note.** Non-negativity is the guard's payoff and is **impossible** in the prior
> guardless design; it is a pure safety invariant (∀-state), independent of confluence,
> so it holds under *every* interleaving even though *which* contended transfer
> succeeds is schedule-dependent.

---

## 7. Fuel / authorization

> **Sublemma 7.1 (B.1 — budget = k funds exactly k same-sig gates).** A depth-k
> same-signature stack provides exactly k tokens on `Σ⟦s⟧` (one materializing per
> firing as `*t` re-releases the depth-(k−1) tail). With `m ≥ k` ready **non-persistent**
> gates on `Σ⟦s⟧`, exactly `min(k,m)` fire and the remaining `m−k` park forever;
> `k=0` (no token stack) ⇒ none fire.
> *Proof.* Induction on chain depth k. **k=0:** no `Σ⟦s⟧`-send ever exists; no rule
> manufactures a send on a channel occurring in no send (COMM/JOIN/REPL only consume
> sends and substitute terms containing none) — by induction on `→`, no `Σ⟦s⟧`-barb is
> reachable, so every gate parks. **k→k:** the stack is `Σ⟦s⟧!(Q)` with `Q` the
> depth-(k−1) chain; exactly one (COMM) consumes the head and one gate; `*@Q ≡ Q`
> (REFL) releases `Q`; the fired gate is consumed (non-persistent) so cannot re-fire;
> apply IH to the residual (depth k−1, m−1 gates). ∎ *(Operationally validated:
> `same_sig_depth2_budget_funds_two_distinct_gates`,
> `same_sig_depth2_budget_meters_out_the_third_identical_gate`.)*

> **Sublemma 7.2 (B.2 — identical-excess ⇒ deterministic observable).** If `m ≥ k`
> ready gates on `Σ⟦s⟧` are pairwise `≡`, the multiset of the `k` fired effects equals
> `k` copies of the common effect, independent of which fired (the parked gate's
> identity is unobservable). *Proof.* By 7.1 exactly `k` fire; all gates `≡`, so any
> choice yields the same effect multiset up to `≡`; the parked ones contribute no barb. ∎

> **Sublemma 7.3 (B.3 — distinct-exact ⇒ all fire).** If `m = k` distinct gates, all
> fire once, order-independent (disjoint redexes against successively-released tokens). ∎

**Application to the scenes** (each request's *fire count* is interleaving-robust —
fixed by static stack depth and gate multiplicity, 7.1):

* **Prod1,2 / Ship1,2 / WS1,2 / Return / Restock1,2 / Fae / Gus / BenBig:** one gate,
  one token (k=m=1) ⇒ each fires exactly once (7.3). For Ship, two compound gates each
  need a `carrier` token; two `carrier :: ()` stacks provide them. For WS, the combined
  token `m1 (*) m2 :: ()` is split by the persistent splitter into the two component
  tokens the compound gate consumes (R3; validated by
  `r3_combined_token_funds_compound_gate_via_splitter`).
* **Ada1,2:** two **distinct** `ada`-gates, depth-2 `ada` budget (k=m=2) ⇒ **both fire**
  (7.3).
* **Ben×3:** three **identical** `ben`-gates, depth-2 `ben` budget (k=2 < m=3) ⇒
  **exactly 2 fire**, the third parks (7.1+7.2); its `ret` is never answered.
* **Cy:** `[cyP -o cyA]` desugars (L5) so `cyP` funds the `for(@q<-@"cyOrder")`
  rendezvous and the inner `swap(…)` is re-signed `cyA`; one `cyP` + one `cyA` token
  ⇒ fires once, emitting one `@"swap"!(…,*ret_Cy)` with `q=2`.
* **Di (ring-fenced):** the thief's gate signs the **free** `diSig` (channel
  `Σ⟦free diSig⟧`); the only token is on the `new`-bound `Σ⟦bound diSig⟧ ≠
  Σ⟦free diSig⟧` (Fact 1) ⇒ k=0 on the gate's channel ⇒ never fires.
* **Eve (unfunded):** no token on `Σ⟦eve⟧` ⇒ k=0 ⇒ never fires.
* **SueDesk (per-clause bind, L6):** one fuel gate on `Σ⟦sueDesk⟧`, one token (k=m=1) ⇒
  the intake gate fires (7.3); the recovered rendezvous `@ord<-@"Sue_orders"` consumes the
  queued order and the body emits the `Sue desk intake` audit. The metered RECEIVE.
* **Deal (Axis-C join, L6):** gated by the product `Σ⟦bidSig⟧, Σ⟦askSig⟧`; both tokens
  present and both messages queued ⇒ it fires once, emitting the `deal matched` audit.
* **ZedDesk (per-clause metering, ⊥):** no token on `Σ⟦zedDesk⟧` ⇒ k=0 ⇒ the intake gate
  never fires; the inner `@ord<-@"Zed_orders"` rendezvous is never installed, so the queued
  order rests unconsumed and no audit is emitted.

> **Corollary 7.4 (which `@"swap"`/`@"move"`/etc. requests are emitted).** Exactly the
> authorized ones: Prod×2, Ship×2, WS×2, Ada×2, **Ben×2** (not 3), Cy×1, Fae×1, Gus×1,
> BenBig×1, Return×1, Restock×2 reach their contract; **Di and Eve emit nothing**, and
> Ben's third micro-request emits nothing. The metered-intake desks operate on their own
> queues: **SueDesk and Deal fire** (each clause funded) and audit, **ZedDesk parks**
> (unfunded) — none touch a ledger cell. ∎

---

## 8. Atomic-swap atomicity

> **Theorem 8.1 (all-or-nothing over BOTH resources).** No reachable state exposes a
> *partial* swap: it is never the case that money moved without goods, or goods without
> money, or some of `{payer,payee,src,dst}` written while others were not.

*Proof.* (i) **Single-step consume.** The swap's `(JOIN_4)` fires as one reduction
consuming all four cells simultaneously ([CAR] §3.3, `rem:db-atomicity`); there is no
reachable state with a strict subset consumed by this join. (ii) **No interleaving
between read and write.** After the join fires, `payer,payee,src,dst` are absent from
the tuplespace; they are re-sent **together** only inside the single resolved `match`
arm (§3). In that window no other step touches them: there is no token to consume, and
SWAP/MOVE/RESTOCK block on absent cells (a join fires only when *all* its cells are
present). Unrelated redexes (other cells, fuel, `match`/arith inside the continuation)
may interleave, but none reads `payer..dst`. (iii) **Uniform write.** §3 + match-
determinism: exactly one arm runs, re-sending all four cells (all post-`true` deltas or
all unchanged). Hence money (payer↔payee) and goods (src↔dst) move together or not at
all. ∎

> **Lemma 8.2 (deadlock-freedom).** No reachable state is stuck with a swap pending while
> all four of its cells carry tokens. *Proof.* `(JOIN_4)` is all-or-nothing — it never
> half-acquires (no "debit payer then block on src"; [CAR] J2). By Lemma 3.1, whenever
> no join is mid-step every cell carries its one token, so the swap is enabled. ∎

> **Theorem 8.3 (bundle vs. chain).** Decomposing a multi-leg transfer into sequential
> single-cell steps admits the partial-funding hazard ([CAR] §4.3 `sec:partial-funding`);
> the single-JOIN swap (8.1) makes it structurally impossible. This is what would let an
> atomic *bundle* of the contention pool collapse it to a unique outcome (§11.3). ∎

---

## 9. Mutual exclusion at the restock lock

> **Theorem 9.1 (binary semaphore).** Let `L(W)` = number of `@"restockLock"` tokens.
> Then `L + (#active restock critical sections) = 1` in every reachable state; hence
> `L ∈ {0,1}`, **at most one** restock CS is active at a time, and every entry is matched
> by an exit re-sending the token.

*Proof.* Induction on `→` with the stated invariant. **Base:** `@"restockLock"!(Nil)`
present, no CS active ⇒ `1 + 0 = 1`. **Step — the only rules changing either count:**
* **ENTER** (`for(_<-@"restockLock")` consumes the token): needs `L≥1`, so by IH
  `L=1,active=0`; after, `L=0,active=1`. Because `L` was 1, only one ENTER can fire (the
  others block on the now-absent token) ⇒ mutual exclusion.
* **EXIT** (a CS arm re-sends `@"restockLock"!(Nil)`): needs `active=1`; after,
  `L=1,active=0`.
* No other rule touches `@"restockLock"` (distinct channel, Fact 1).
**Liveness:** each entered CS is straight-line (read `Whse_stk`, announce, one `match`
arm, the inner `Sam_stk` update which is always re-enabled by Lemma 3.1) and terminates
(§12), and the EXIT send is unconditional in both arms ⇒ the token always returns ⇒ the
two restocks **serialize** (run one-at-a-time, in some order). ∎ *(Empirically the two
CSs read warehouse snapshots 34 then 25 — distinct, confirming they did not interleave.)*

---

## 10. Response-faithfulness

> **Lemma 10.1.** For each caller `c`: `RESPONSE_c = true` iff its operation applied;
> `= false` iff its guard failed (cells unchanged); `= ⊥` iff its fuel gate never fired.

*Proof.* `ret_c` is a fresh `new`-bound channel; its **only** producer is the one
contract firing on `c`'s request (Corollary 7.4 ⇒ at most one such firing; the contract
consumes the request once). The contract emits `ret_c!(b)` only inside the `match` arm
selected by the guard (match-determinism): the `true` arm emits `ret_c!(true)` together
with the applied deltas; the `false` arm emits `ret_c!(false)` together with the
unchanged re-sends. Hence `true ⇔ applied`, `false ⇔ unchanged`. If `c`'s request emits
nothing (Di ring-fenced / Eve unfunded / Ben's parked third micro — Cor. 7.4), no `ret_c!`
is ever produced, so `for(@ok<-ret_c){…}` blocks forever ⇒ `⊥`. Single-valued: ≤ 1
producer. ∎

> **Empirical confirmation (this reducer).** `true`: Prod×2, Ship×2, WS×2, Ada×2,
> Ben-micro×2, Cy, Restock×2, the contention winner, **SueDesk intake, the Deal match**.
> `false`: BenBig, the contention loser. `⊥` (no audit line, gate parked in the dump):
> Di, Eve, Ben's third micro, **ZedDesk** (its `@"Zed_orders"` order resting unconsumed).

---

## 11. Contention outcome-space

**Setup.** `Flash_stk` opens at 1. Two funded swaps compete:
`F = swap(Fae,Sue,Flash_stk,Fae_home,20,1)` and
`G = swap(Gus,Sue,Flash_stk,Gus_home,20,1)`. Both gate-fire (each has its token, §7);
both can afford ($30 ≥ 20). They share the cells `@"Flash_stk"` and `@"Sue_cash"`.

> **Theorem 11.1 (two-outcome characterization).** The reachable normal forms of this
> sub-system are **exactly two**, up to `≡`: **(G-wins)** `RESPONSE_G=true` (Gus −20,
> Sue +20, Flash 1→0, Gus_home +1) and `RESPONSE_F=false` (Fae unchanged); and
> **(F-wins)**, symmetric. In both, conservation (§4,§5) and non-negativity (§6) hold,
> and **exactly one** of `{F,G}` gets `true`.

*Proof.* `@"Flash_stk"` carries one token (Lemma 3.1). `F` and `G` each need it in their
4-cell join; the single token serializes them (same mechanism as §9's lock) — exactly
one join fires first. **Case first = G:** `G` reads `ss = 1 ≥ 1`, applies (Flash→0,
Gus_home→1, Gus−20, Sue+20), re-sends `@"Flash_stk"!(0)`; then `F` fires, reads `ss=0`,
guard `0 ≥ 1` false ⇒ re-sends unchanged, `RESPONSE_F=false`. → **(G-wins)**. **Case
first = F:** symmetric → **(F-wins)**. **No third outcome:** both swaps resolve (SN §12;
deadlock-free §8.2). *Both-succeed* is unreachable — it would require Flash to be
debited twice from 1, contradicting non-negativity (§6). *Both-fail* is unreachable —
the first join to fire reads `Flash=1 ≥ 1`, whose guard holds, so it succeeds. Each
outcome is invariant-valid by §4–§6, §10. ∎

> **Theorem 11.2 (general contention).** For any pool of guarded transfers contending on
> a scarce cell, **every** reachable normal form corresponds to **some** serialization
> `σ` (induced by the single token, which forbids simultaneous firing) in which each
> transfer succeeds iff the cell still covers it at its position in `σ`; the successful
> set is the **greedy-by-arrival fit** of `σ`. Conversely every such fit is reachable.
> Safety (§4–§6) holds at every prefix.
> *Proof.* The interleaving linearly orders the joins touching the cell (no two fire at
> once); along `σ` each `match` scrutinee is deterministic given the committed prefix
> (§1.2), so success = "remaining ≥ demand," i.e. greedy-by-arrival. ∎
> **(R-5, kept rigorous.)** This is *existential* over serializations and uses
> **greedy-by-arrival**, NOT a global maximum-weight fit; we claim only "∃σ whose greedy
> fit = the successful set, and every greedy fit is reachable," not optimality.

> **Theorem 11.3 (atomic bundle collapses the pool).** Had `F,G` been bundled into one
> N-ary transaction with a combined guard, the pool would have a **unique** outcome
> (8.1 + a single combined scrutinee evaluated once): all-or-none, no serialization
> choice. ∎

> **Definition 11.4 ("correct under every interleaving").** (Safety) Invariants §3–§10
> hold in all reachable states, hence under every schedule. (Valid-outcome-set) The set
> of reachable normal forms = the characterized family (11.1/11.2): a quotient of the
> interleaving space — finitely many invariant-valid outcomes. (Collapse) Non-contending
> transfers commute (disjoint redexes); only genuinely contended cells produce
> multiplicity.

> **Empirical note (honesty).** The rho calculus permits **either** `F`-wins or `G`-wins
> (Thm 11.1). The `rholang-cli` reducer resolves this race **deterministically** — across
> repeated runs Gus wins every time. This does not contradict the theorem (which
> characterizes the *reachable* family under all schedulers); it reflects that this one
> reducer's matching order is fixed. The provable, run-invariant facts are: exactly one
> claimant wins (no double-spend of the unit), and conservation/non-negativity hold.

---

## 12. Termination (strong normalization)

> **Theorem 12.1.** `𝒯⟦D⟧` strongly normalizes.

*Proof.* Define a weight `μ` decreasing on every rule and bounded below, summing
(with top-down dominating weights so each producer outweighs everything it can spawn):
`μ = w₁·(authorization COMMs/JOINs still possible) + w₂·(SWAP/MOVE/PRODUCE/RESTOCK
instances not yet fired) + w₃·(restock competitors not yet completed) + w₄·(pending
@"swap"/@"move"/@"produce"/@"restock" requests) + w₅·(pending @"display"/@"audit"/ret
sends) + w₆·(match redexes) + w₇·(ground arith/bool eval redexes) + w₈·(pending
@"cyOrder"/lollipop-chain steps)`.
* Each summand is finite: finitely many fuel tokens (7.1 — each token stack has finite depth;
  parked gates contribute 0 going forward); finitely many contract instances (each
  request consumed once, Cor. 7.4); **restock counted as competitors-not-yet-completed**
  (a fixed finite set — each enters once and terminates — so the recirculating lock token
  does not make this unbounded, **R-6**).
* Each rule strictly decreases `μ`: a fuel COMM ↓w₁ (may emit a request ↑w₄, dominated by
  `w₁`); a contract JOIN ↓w₂ (emits display/ret/match, dominated); `match` ↓w₆; eval ↓w₇;
  console/ret REPL ↓w₅; restock ENTER advances a competitor toward completion ↓w₃, EXIT
  re-sends the lock without increasing competitors-remaining. No rule raises `μ` overall. ∎

SN feeds §11 (every reduction reaches a normal form, so the outcome family is total) and
§9 (lock liveness). It does **not** feed a confluence claim.

---

## 13. Main theorem

> **Theorem 13.1 (ecosystem correctness — safety + valid-outcome-set).** Under **every**
> interleaving, `𝒯⟦D⟧`:
> (a) conserves money `T_cash = 410` (§4);
> (b) conserves inventory `T_stock = 67 + produced = 83` at any normal form (§5);
> (c) keeps every cash and stock cell **≥ 0** (§6);
> (d) is **response-faithful** — every `RESPONSE_c ∈ {true,false,⊥}` exactly classifies
>     applied / guard-denied / fuel-denied (§10);
> (e) executes every purchase as an **atomic money↔goods swap** — no partial application
>     reachable (§8);
> (f) **mutually excludes** the restock critical sections (§9);
> (g) **enforces fuel budgets** — exactly the authorized count of each request fires;
>     Ben's third micro-buy, Di's drain, and Eve's theft fire **zero** (§7);
> and **strongly normalizes** (§12). The set of reachable **final states** equals exactly
> the invariant-valid family of §11 — multiplicity arises **only** from the flash-sale
> pool (one winner), and an atomic bundle would make even that unique. This is a safety +
> valid-outcome-set guarantee, **not** a unique-normal-form claim.

> **A representative reachable outcome** (this reducer; one normal form of the family),
> with the verified final ledgers:
> ```
> CASH:  Ada 79  Ben 52  Cy 40  Di 40  Sue 85  Sam 46  Whse 28  Fae 30  Gus 10     (Σ 410)
> STOCK: Fab1 0  Fab2 0  Whse_stk 20  Sue_stk 22  Sam_stk 30  Flash 0
>        Ada_home 6  Ben_home 2  Cy_home 2  Fae_home 0  Gus_home 1                  (Σ 83)
> ```
> *Derivation.* Sue cash `50 +15(Ada3) +10(Ada2) +10(Cy) +20(flash winner) −16(wholesale)
> −4(refund) = 85`; Whse_stk `24 +10 +6(ship) −8 −6(wholesale) −6(restock) = 20`;
> Ada_home `2 +3 +2 −1(return) = 6`; etc. The **other** family member swaps Fae↔Gus
> (Fae cash 10 / home 1, Gus cash 30 / home 0); all eight other accounts and all other
> stock cells are identical, and both totals stay `410`/`83`. ∎

**"No sequential assumption", precisely.** The safety invariants (a)–(g) are
∀-reachable-state and thus schedule-independent; the only ordering constraints are
intra-request fuel-gated rendezvous, the atomic JOINs, and the restock lock — never the
textual order of the flat parallel parts. The single source of observable nondeterminism
is the flash-sale pool (§11).

---

## 14. Source-rules and adequacy

> **Corollary 14.1.** Each authorization realizes a source rule: ground `[ada]`,`[faeSig]`,…
> ⇒ **R1**; `[fab_i (*) carrier]`,`[m1 (*) m2]` ⇒ **R2/R3** (combined token via the splitter);
> `[# "Fab_i"]` ⇒ a section-signed R1; `[cyP -o cyA]` ⇒ the **lollipop** sugar then R1 at
> each gate; the per-clause binds `[sueDesk]` and the join `[bidSig] & [askSig]` ⇒ the
> **Axis-C join** (each clause's rendezvous funded independently; L6/`lower_signed_join`);
> the `new`-bound `diSig` fuel (a free-`diSig` thief gate), the unfunded `[eve]`, and the
> unfunded `[zedDesk]` have **no** enabling cost step (§7). Cite [CAR] `eq:rule1`–`eq:rule3`,
> `def:sugar-lollipop`, the Axis-C join rule, and the Verification Sketch
> (`app:verification`, `prop:subst-commute`).
> *Proof.* §7's COMM/JOIN chains are the right-hand sides of the §A verification per rule. ∎

> **Corollary 14.2 (adequacy — layer-local).** By Graded Adequacy ([GSLT] `thm:adequacy`),
> the cost-accounted reduction matches the source rules up to **quote-faithful
> bisimulation**; so *which* operations are authorized, and on which signatures, is a
> bisimulation invariant, independent of schedule.
> **(R-7.)** Adequacy speaks to the **authorization layer** (fuel ↔ source rules); it is
> **orthogonal** to the ledger-level nondeterminism (the flash-sale race lives in the
> target-level `match` on a schedule-dependent read, §11). Adequacy does **not** assert a
> unique ledger outcome. ∎

---

## 15. Remarks

* **Overdraft guards — now PRESENT, on purpose (reverses the prior §7).** The prior
  payment-ledger omitted guards to keep additive commutativity and confluence; this demo
  adds them to enforce **non-negativity** (§6) and to demonstrate genuine **contention**
  (§11). The cost is confluence; the gain is a real safety property and an honest
  nondeterministic-but-correct race.
* **Dual resource, two conservation laws.** Money is globally conserved (§4); inventory is
  conserved *after production* (§5) — factories are the only source. A purchase is the one
  operation that touches both at once, atomically (§8).
* **Atomic JOIN vs. mutex lock.** The atomic 4-cell swap (§8) handles all multi-cell
  exchanges that fit one rendezvous (purchase, wholesale, return) — deadlock-free, no lock
  needed. The lock (§9) is reserved for the **multi-step** restock critical section
  (read → announce → conditional write), which cannot be one JOIN because the announce
  must reflect the pre-write state.
* **Implementation vs paper §A.** The interleaved compound gate and chained splitter are
  this repository's normalizer output (validated by `cost_accounting_reduction_spec.rs`),
  denoting the same semantics as [CAR] §A.
* **This reducer is internally deterministic on the race (§11 empirical note).** The
  calculus permits either flash-sale winner; `rholang-cli` happens to pick one. The
  run-invariant guarantees are the safety invariants and "exactly one winner."
